//! Queues first-use pipeline variants as soon as a view exists, so loading absorbs compilation.

use bevy::prelude::IntoScheduleConfigs;
use render_scene::PipelineWarmupReadiness;

use std::{any::type_name, collections::HashSet, marker::PhantomData};

#[cfg(test)]
use bevy::prelude::{Handle, Mut, Shader, World, default};
use bevy::{
    prelude::{
        App, BevyError, Has, Local, Msaa, Query, Res, ResMut, Resource, Result, SubApp, SystemSet,
        error, info, warn,
    },
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{
            CachedPipelineState, CachedRenderPipelineId, PipelineCache, TextureFormat,
        },
        view::ViewTarget,
    },
    shader::ShaderCacheError,
};

/// Render frames to wait after the critical gate opens before GL starts background batches.
const GL_BACKGROUND_SETTLE_FRAMES: u32 = 2;

/// The view properties our built-in pipeline keys depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WarmView {
    pub msaa: Msaa,
    pub hdr: bool,
    pub enhanced: bool,
    pub shader_pack: bool,
    /// The surface or image format the view finally writes; absent until its target exists.
    pub output: Option<TextureFormat>,
}

pub(crate) type WarmupIds = Vec<CachedRenderPipelineId>;

/// When an owner's variants must exist.
///
/// `Critical` owners gate world entry. `Background` owners may compile after the first
/// world frame, so first-use of an uncommon effect can happen while gameplay continues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WarmupTier {
    Critical,
    Background,
}

/// Enumerates active coverage and Performance's single-sample path without duplicates.
pub(crate) fn sample_modes(current: Msaa) -> impl Iterator<Item = Msaa> {
    [Some(current), (current != Msaa::Off).then_some(Msaa::Off)]
        .into_iter()
        .flatten()
}

/// Implemented by each pipeline owner so warmup fills the same memoized variants drawing uses.
pub(crate) trait PrewarmPipelines:
    Resource<Mutability = bevy::ecs::component::Mutable>
{
    /// Pushes the ID of every variant a draw for `view` may later request.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError>;
}

/// Implemented by owners that compile outside the pipeline cache, as their inputs appear.
pub(crate) trait PendingPipelines: Resource {
    /// Whether a pipeline a draw may request this frame is still compiling.
    fn pending(&self) -> bool;
}

#[derive(Resource, Default)]
struct WarmupRegistry {
    views: Vec<WarmView>,
    /// Critical variants that hold loading until every one is usable.
    ids: WarmupIds,
    /// Variants compiled once the view exists; readiness never waits for these.
    background_ids: WarmupIds,
    failed: HashSet<CachedRenderPipelineId>,
    /// Owned compiles still running this frame.
    pending: bool,
    /// Registration order for the one-owner-at-a-time GL background schedule.
    background_owners: usize,
    /// Incremented on view changes so per-owner caches discard old view keys.
    views_generation: u64,
    background: BackgroundWarmup,
}

impl WarmupRegistry {
    /// Drops the old view's background queue before the next cycle can use its IDs.
    fn restart_background(&mut self) {
        self.background_ids.clear();
        // Terminal errors are only logged once per ID. Keep critical errors to avoid repeating
        // them every time the view changes, but forget background IDs that no longer matter.
        let critical = &self.ids;
        self.failed.retain(|id| critical.contains(id));
        self.views_generation = self.views_generation.wrapping_add(1);
        self.background.restart_cycle();
    }
}

/// Serializes GL background owners after the critical gate has been released.
#[derive(Default)]
struct BackgroundWarmup {
    /// Incremented once per render frame by [`collect_views`].
    frame: u64,
    /// Registration slot of the next owner whose variants may be queued.
    cursor: usize,
    /// Slot waiting for its queued variants to finish compiling.
    active: Option<usize>,
    /// Frame of the last batch transition; prevents two owners starting in one frame.
    transition_frame: Option<u64>,
    /// Whether readiness has been observed since it last closed.
    armed: bool,
    countdown: u32,
    /// Whether background batches may start this frame.
    ready: bool,
}

impl BackgroundWarmup {
    /// Opens background work only after the critical gate has stayed ready for a short settle.
    ///
    /// Native backends keep the existing eager behavior and are always allowed to queue.
    fn observe(&mut self, critical_ready: bool, gl: bool) {
        self.frame = self.frame.wrapping_add(1);
        if !gl {
            self.ready = true;
            return;
        }
        if !critical_ready {
            self.armed = false;
            self.countdown = GL_BACKGROUND_SETTLE_FRAMES;
            self.ready = false;
            return;
        }
        if self.ready {
            return;
        }
        if !self.armed {
            self.armed = true;
            self.countdown = GL_BACKGROUND_SETTLE_FRAMES;
            return;
        }
        if self.countdown > 0 {
            self.countdown -= 1;
            return;
        }
        self.ready = true;
    }

    /// Revisits every background owner when the view keys change.
    fn restart_cycle(&mut self) {
        self.cursor = 0;
        self.active = None;
        self.transition_frame = None;
        // The new critical set must settle before background work may start, and the old
        // view's already-open gate must not leak into the new one.
        self.armed = false;
        self.countdown = GL_BACKGROUND_SETTLE_FRAMES;
        self.ready = false;
    }

    /// Whether this slot already had its turn in the current frame.
    fn transitioned_this_frame(&self) -> bool {
        self.transition_frame == Some(self.frame)
    }
}

/// Per-background-owner memo that is discarded whenever the view keys change.
#[derive(Default)]
struct WarmedViews {
    generation: u64,
    views: Vec<WarmView>,
}

impl WarmedViews {
    fn for_generation(&mut self, generation: u64) -> &mut Vec<WarmView> {
        if self.generation != generation {
            self.generation = generation;
            self.views.clear();
        }
        &mut self.views
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum WarmupSet {
    Views,
    Owners,
}

#[derive(Resource)]
struct WarmupOwner<T> {
    tier: WarmupTier,
    slot: usize,
    _marker: PhantomData<T>,
}

impl<T> WarmupOwner<T> {
    fn new(tier: WarmupTier, slot: usize) -> Self {
        Self {
            tier,
            slot,
            _marker: PhantomData,
        }
    }
}

/// Registers an owner whose variants must be ready before loading releases. Idempotent, so
/// plugins that install from both `build` and `finish` may call it twice.
pub(crate) fn register<T: PrewarmPipelines>(app: &mut App) {
    register_tier::<T>(app, WarmupTier::Critical);
}

/// Registers an owner whose variants may compile after the world is visible. Idempotent.
pub(crate) fn register_background<T: PrewarmPipelines>(app: &mut App) {
    register_tier::<T>(app, WarmupTier::Background);
}

fn register_tier<T: PrewarmPipelines>(app: &mut App, tier: WarmupTier) {
    let Some(render_app) = install(app) else {
        return;
    };
    if render_app.world().contains_resource::<WarmupOwner<T>>() {
        let existing = render_app.world().resource::<WarmupOwner<T>>().tier;
        if existing != tier {
            warn!(
                "pipeline warmup owner {}: already registered as {existing:?}; keeping that tier",
                type_name::<T>()
            );
        }
        return;
    }
    let slot = match tier {
        WarmupTier::Critical => usize::MAX,
        WarmupTier::Background => {
            let mut registry = render_app.world_mut().resource_mut::<WarmupRegistry>();
            let slot = registry.background_owners;
            registry.background_owners += 1;
            slot
        }
    };
    render_app.insert_resource(WarmupOwner::<T>::new(tier, slot));
    info!("pipeline warmup owner {}: {tier:?}", type_name::<T>());
    if tier == WarmupTier::Background {
        render_app.add_systems(
            Render,
            prewarm_background_owner::<T>
                .in_set(WarmupSet::Owners)
                .run_if(render_scene::startup_work::world_work_allowed),
        );
    } else {
        render_app.add_systems(
            Render,
            prewarm_owner::<T>
                .in_set(WarmupSet::Owners)
                .run_if(render_scene::startup_work::world_work_allowed),
        );
    }
}

/// Holds readiness while `T` reports a compile in flight; idempotent like [`register`].
pub(crate) fn register_pending<T: PendingPipelines>(app: &mut App) {
    let Some(render_app) = install(app) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupOwner<T>>() {
        render_app
            .insert_resource(WarmupOwner::<T>::new(WarmupTier::Critical, usize::MAX))
            .add_systems(
                Render,
                owner_pending::<T>
                    .run_if(render_scene::startup_work::world_work_allowed)
                    .in_set(RenderSystems::Cleanup)
                    .before(publish_readiness),
            );
    }
}

/// Holds world-entry readiness while an asynchronous GPU owner is not yet installed.
pub(crate) fn register_pending_resource<T: Resource>(app: &mut SubApp) {
    app.init_resource::<WarmupRegistry>();
    app.add_systems(
        Render,
        pending_resource::<T>
            .in_set(RenderSystems::Cleanup)
            .before(publish_readiness),
    );
}

/// Folds the existence of an unfinished initializer into the same pipeline-readiness gate.
fn pending_resource<T: Resource>(pending: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= pending.is_some();
}

/// Installs shared scene formats and readiness tracking for custom pipeline owners.
fn install(app: &mut App) -> Option<&mut SubApp> {
    app.init_resource::<PipelineWarmupReadiness>();
    app.init_resource::<render_scene::startup_work::WorldWorkAdmission>();
    let admission = app
        .world()
        .resource::<render_scene::startup_work::WorldWorkAdmission>()
        .clone();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let render_app = app.get_sub_app_mut(RenderApp)?;
    render_app.insert_resource(admission);
    crate::scene_target::install_formats(render_app);
    if !render_app.world().contains_resource::<WarmupRegistry>() {
        render_app
            .insert_resource(shared)
            .init_resource::<WarmupRegistry>()
            // Bevy's upscaling blocks on its blit pipeline in `Prepare`, and that wait starts
            // every queued pipeline first; their tasks share one shader-cache lock. Queuing
            // after it lets the blit compile alone, and this frame's `Render` set still starts
            // the warm-up.
            .configure_sets(
                Render,
                (WarmupSet::Views, WarmupSet::Owners)
                    .chain()
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
            )
            .add_systems(Render, collect_views.in_set(WarmupSet::Views))
            .add_systems(Render, publish_readiness.in_set(RenderSystems::Cleanup));
        info!(
            "pipeline warmup: critical owners gate loading; background owners {}",
            if !crate::gl_compat::enabled() {
                "queue during loading too, without gating readiness"
            } else if crate::gl_compat::compile_context_enabled() {
                "are deferred until readiness on ANGLE's separate compile context"
            } else {
                "are deferred until readiness and compile synchronously in small batches"
            }
        );
    }
    Some(render_app)
}

/// The camera state that selects each view's pipeline variants.
type ViewKeys<'w, 's> = Query<
    'w,
    's,
    (
        &'static bevy::render::camera::ExtractedCamera,
        &'static Msaa,
        Option<&'static ViewTarget>,
        Option<&'static render_scene::EnhancedRendering>,
        Has<render_scene::shaderpack::ShaderPackRendering>,
    ),
>;

fn collect_views(
    views: ViewKeys,
    shared: Res<PipelineWarmupReadiness>,
    admission: Res<render_scene::startup_work::WorldWorkAdmission>,
    mut registry: ResMut<WarmupRegistry>,
) {
    let mut collected = Vec::new();
    for (camera, msaa, target, enhanced, shader_pack) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: camera.hdr,
            enhanced: enhanced.is_some(),
            shader_pack,
            output: target.and_then(ViewTarget::out_texture_view_format),
        };
        if !collected.contains(&key) {
            collected.push(key);
        }
    }
    if collected != registry.views {
        registry.views = collected;
        registry.restart_background();
    }
    registry.pending = false;
    // Readiness may already be true from the menu before world work is admitted. Require
    // admission too, so GL backgrounds cannot start on the menu's empty critical set.
    registry.background.observe(
        admission.is_admitted() && shared.is_ready(),
        crate::gl_compat::enabled(),
    );
}

/// Queues every not-yet-warmed variant of one owner for the current view keys.
fn warm_owner_views<T: PrewarmPipelines>(
    owner: &mut T,
    cache: &PipelineCache,
    views: &[WarmView],
    warmed: &mut Vec<WarmView>,
    ids: &mut WarmupIds,
) {
    for &view in views {
        for msaa in sample_modes(view.msaa) {
            let view = WarmView { msaa, ..view };
            if warmed.contains(&view) {
                continue;
            }
            // The draw path would fail the same way, so the error does not hold loading.
            if let Err(error) = owner.prewarm(cache, view, ids) {
                error!("pipeline prewarm {}: {error}", type_name::<T>());
            }
            warmed.push(view);
        }
    }
}

/// Specializes once per view configuration; later frames only compare keys.
fn prewarm_owner<T: PrewarmPipelines>(
    owner: Option<ResMut<T>>,
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    mut warmed: Local<Vec<WarmView>>,
    mut skipped: Local<bool>,
    mut reported: Local<bool>,
) {
    // An owner that never initialised has nothing it could draw.
    let Some(mut owner) = owner else {
        if !*skipped {
            info!(
                "pipeline warmup owner {}: skipped, resource not installed",
                type_name::<T>()
            );
            *skipped = true;
        }
        return;
    };
    let WarmupRegistry { views, ids, .. } = &mut *registry;
    let before = ids.len();
    warm_owner_views(&mut *owner, &cache, views, &mut warmed, ids);
    if !*reported && !views.is_empty() {
        info!(
            "pipeline warmup owner {}: critical, queued {} variants",
            type_name::<T>(),
            ids.len() - before
        );
        *reported = true;
    }
}

/// Background owners follow the backend policy instead of the loading gate.
///
/// Native backends queue them during loading exactly as before, but their IDs stay out of
/// readiness. GL defers them until critical readiness and then allows one owner per frame.
fn prewarm_background_owner<T: PrewarmPipelines>(
    marker: Res<WarmupOwner<T>>,
    owner: Option<ResMut<T>>,
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    mut warmed: Local<WarmedViews>,
    mut skipped: Local<bool>,
    mut reported: Local<bool>,
) {
    let Some(mut owner) = owner else {
        if !*skipped {
            info!(
                "pipeline warmup owner {}: background skipped, resource not installed",
                type_name::<T>()
            );
            *skipped = true;
        }
        // A missing owner must not strand every later background slot on GL.
        if crate::gl_compat::enabled() {
            let WarmupRegistry {
                background_ids,
                failed,
                background,
                ..
            } = &mut *registry;
            if !background.ready || background.transitioned_this_frame() {
                return;
            }
            if background.active == Some(marker.slot) {
                if ids_ready(&cache, background_ids, failed) {
                    background.active = None;
                    background.cursor += 1;
                    background.transition_frame = Some(background.frame);
                }
            } else if background.active.is_none() && background.cursor == marker.slot {
                background.cursor += 1;
                background.transition_frame = Some(background.frame);
            }
        }
        return;
    };
    let warmed = warmed.for_generation(registry.views_generation);
    if !crate::gl_compat::enabled() {
        let WarmupRegistry {
            views,
            background_ids,
            ..
        } = &mut *registry;
        let before = background_ids.len();
        warm_owner_views(&mut *owner, &cache, views, warmed, background_ids);
        if !*reported && !views.is_empty() {
            info!(
                "pipeline warmup owner {}: background queued during loading; {} variants, readiness unaffected",
                type_name::<T>(),
                background_ids.len() - before
            );
            *reported = true;
        }
        return;
    }

    let slot = marker.slot;
    let WarmupRegistry {
        views,
        background_ids,
        failed,
        background,
        ..
    } = &mut *registry;
    if !background.ready || background.transitioned_this_frame() {
        return;
    }
    if let Some(active) = background.active {
        if active != slot {
            return;
        }
        if !ids_ready(&cache, background_ids, failed) {
            return;
        }
        background.active = None;
        background.cursor += 1;
        background.transition_frame = Some(background.frame);
        info!(
            "pipeline warmup owner {}: background batch finished",
            type_name::<T>()
        );
        return;
    }
    if background.cursor != slot {
        return;
    }

    // Only this batch's IDs gate the next transition. Keeping IDs from earlier owners (or a
    // previous view) would let one old `ShaderNotLoaded` entry block every later batch.
    background_ids.clear();
    warm_owner_views(&mut *owner, &cache, views, warmed, background_ids);
    let queued = background_ids.len();
    if queued == 0 {
        background.cursor += 1;
        background.transition_frame = Some(background.frame);
        info!(
            "pipeline warmup owner {}: background already warm; skipping batch",
            type_name::<T>()
        );
        return;
    }
    background.active = Some(slot);
    info!(
        "pipeline warmup owner {}: queued background batch of {queued} variants",
        type_name::<T>()
    );
}

fn owner_pending<T: PendingPipelines>(owner: Option<Res<T>>, mut registry: ResMut<WarmupRegistry>) {
    registry.pending |= owner.is_some_and(|owner| owner.pending());
}

/// Publishes the critical gate only; background batches keep running after it opens.
fn publish_readiness(
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    shared: Res<PipelineWarmupReadiness>,
) {
    let ready = registered_pipelines_ready(&cache, &mut registry);
    shared.set_ready(ready);
}

/// Shader loads keep loading held; terminal compile errors are logged once and stop blocking.
///
/// Background IDs are deliberately absent here: they must never hold world entry.
fn registered_pipelines_ready(cache: &PipelineCache, registry: &mut WarmupRegistry) -> bool {
    let WarmupRegistry {
        views,
        ids,
        failed,
        pending,
        ..
    } = registry;
    let mut ready = !views.is_empty() && !*pending;
    ready &= ids_ready(cache, ids, failed);
    ready
}

/// Whether every ID is usable, folding terminal errors into readiness exactly once.
fn ids_ready(
    cache: &PipelineCache,
    ids: &[CachedRenderPipelineId],
    failed: &mut HashSet<CachedRenderPipelineId>,
) -> bool {
    let mut ready = true;
    for &id in ids {
        ready &= match cache.get_render_pipeline_state(id) {
            CachedPipelineState::Ok(_) => true,
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => false,
            CachedPipelineState::Err(error) => {
                if failed.insert(id) {
                    error!("pipeline prewarm {id:?}: {error}");
                }
                true
            }
            _ => false,
        };
    }
    ready
}

#[cfg(test)]
#[path = "pipeline_warmup/tests.rs"]
mod tests;

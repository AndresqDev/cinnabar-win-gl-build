# Potion tooltips

Potion stacks carry their potion type in the item auxiliary value. The
inventory tooltip and selected-item HUD name the type, and the tooltip lists the
effects it grants.

## Rules

| Rule | Behavior |
| --- | --- |
| Name | `potion.<type>.name`, `.splash.name` or `.linger.name` for potion, splash and lingering stacks; a custom name still wins. |
| Effect line | Effect name, then the `potion.potency.<amplifier>` numeral when the amplifier is above zero, then `(m:ss)` unless the effect is instant. |
| Colour | Blue (`§9`) for beneficial effects, red (`§c`) for harmful ones, following each effect's pinned harmful flag. |
| Splash duration | Equal to the drinkable duration. |
| Unknown type | An auxiliary value outside the vanilla table keeps the generic item name and adds no effect lines. |

The potion table lives in `crates/assets/src/potion.rs`. Tests check its
identifiers against the pinned `mojang-potion-effects.json` inventory and its
name keys against the pinned `en_US.lang`.

## Sources and limits

The issue's vanilla capture shows `Splash Potion of Fire Resistance` with a blue
`Fire Resistance (8:00)` line for the long variant. Amplifiers and durations
follow the Minecraft wiki's Bedrock potion tables: Regeneration II and Poison II
last 450 ticks (0:22), Decay grants Wither II for 0:40, and Turtle Master II
grants Slowness VI and Resistance IV.

The following are provisional and remain incomplete in `plan.md`:

- Lingering effect durations are divided by four, as the cloud applies them, without native tooltip evidence.
- Base potions show no `No Effects` line.
- Tipped arrows keep their generic name.
- Attribute modifier lines are not shown.

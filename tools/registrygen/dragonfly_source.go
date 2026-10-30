package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
)

func collectDragonflyStates(registry world.BlockRegistry) ([]SourceState, error) {
	if registry == nil {
		return nil, errors.New("block registry is nil")
	}
	registry.Finalize()
	blocks := registry.Blocks()
	if len(blocks) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(blocks), maxRecordCount)
	}
	states := make([]SourceState, 0, len(blocks))
	for rid, value := range blocks {
		name, properties := value.EncodeBlock()
		networkHash, ok := registry.RuntimeIDToHash(uint32(rid))
		if !ok {
			return nil, fmt.Errorf("runtime ID %d has no network hash", rid)
		}
		typed, err := typedProperties(properties)
		if err != nil {
			return nil, fmt.Errorf("runtime ID %d: %w", rid, err)
		}
		states = append(states, SourceState{Name: name, Properties: typed, Ordinal: uint32(rid), NetworkHash: networkHash, Flags: classifyFlags(value)})
	}
	return states, nil
}

func collect(registry world.BlockRegistry) ([]Record, error) {
	states, err := collectDragonflyStates(registry)
	if err != nil {
		return nil, err
	}
	records := make([]Record, 0, len(states))
	for _, state := range states {
		record, err := classifyRecord(state)
		if err != nil {
			return nil, fmt.Errorf("classify runtime ID %d: %w", state.Ordinal, err)
		}
		record.SequentialID = state.Ordinal
		record.NetworkHash = state.NetworkHash
		record.Flags = state.Flags
		record.Provenance = ProvenanceDragonfly
		finalizeGeometryFacts(&record)
		records = append(records, record)
	}
	return records, nil
}

func validRecordFlags(flags uint8) bool {
	if flags&^allBlockFlags != 0 {
		return false
	}
	air := flags&flagAir != 0
	cube := flags&flagCubeGeometry != 0
	occludes := flags&flagOccludesFullFace != 0
	leaf := flags&flagLeafModel != 0
	return (!air || flags == flagAir) && (!occludes || cube) && (!leaf || (cube && !occludes))
}

func classifyFlags(value world.Block) uint8 {
	name, properties := value.EncodeBlock()
	if name == "minecraft:air" {
		return flagAir
	}
	// These experimental protocol-1001 blocks were unknown when the reviewed
	// BREG1003 was produced. The light-metadata dependency pin now implements
	// them as solids, but this light-only change must preserve the committed
	// visual registry's established no-geometry facts.
	switch name {
	case "minecraft:polished_sulfur", "minecraft:sulfur_bricks",
		"minecraft:chiseled_sulfur", "minecraft:cinnabar_bricks",
		"minecraft:cinnabar", "minecraft:chiseled_cinnabar",
		"minecraft:polished_cinnabar", "minecraft:sulfur":
		return 0
	}
	// Dragonfly's registered zero-value ShulkerBox carries an uninitialised
	// animation counter and its Model method panics. It is a non-cube model,
	// so preserve the existing no-full-face BREG facts without calling Model.
	if _, ok := value.(block.ShulkerBox); ok {
		return 0
	}
	switch value.Model().(type) {
	case model.Leaves:
		return flagCubeGeometry | flagLeafModel
	case model.Solid:
		return flagCubeGeometry | flagOccludesFullFace
	}

	// BasicBlockRegistry uses the high half returned by Hash as its public
	// unknownBlock discriminator: math.MaxUint64 means no concrete block
	// implementation is registered. Its unknownModel deliberately looks like a
	// full cube, so model geometry cannot safely classify these states.
	_, stateHash := value.Hash()
	if stateHash == math.MaxUint64 && approvedUnknownFullCubeState(name, properties) {
		return flagCubeGeometry | flagOccludesFullFace
	}
	return 0
}

func approvedUnknownFullCubeState(name string, properties map[string]any) bool {
	switch name {
	case "minecraft:mycelium":
		return len(properties) == 0
	case "minecraft:red_mushroom_block", "minecraft:brown_mushroom_block", "minecraft:mushroom_stem":
		if len(properties) != 1 {
			return false
		}
		bits, ok := properties["huge_mushroom_bits"].(int32)
		return ok && bits >= 0 && bits <= 15
	default:
		return false
	}
}

func canonicalJSON(properties map[string]any) ([]byte, error) {
	return json.Marshal(properties)
}

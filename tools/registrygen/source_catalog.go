package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

func generateRegistry(pmmpRoot, prismarineRoot, coverageManifestPath string, registry world.BlockRegistry) ([]Record, RegistryMetadata, GenerationReport, error) {
	protocolPath := filepath.Join(pmmpRoot, "protocol_info.json")
	pmmpPalettePath := filepath.Join(pmmpRoot, "canonical_block_states.nbt")
	prismarineStatesPath := filepath.Join(prismarineRoot, "blockStates.json")
	prismarineShapesPath := filepath.Join(prismarineRoot, "blockCollisionShapes.json")

	protocol, err := readProtocol(protocolPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if protocol != registryProtocol {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("PMMP protocol %d does not match required %d", protocol, registryProtocol)
	}
	pmmp, err := readNBTStates(pmmpPalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("read PMMP palette: %w", err)
	}
	dragonfly, err := collectDragonflyStates(registry)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarine, err := readPrismarineStates(prismarineStatesPath, prismarineShapesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	joined, err := joinSources(pmmp, dragonfly, prismarine, canonicalStateHash)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if len(joined) != 16_913 {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("canonical state count %d does not match required 16913", len(joined))
	}
	nameCount := uniqueNameCount(pmmp)
	if nameCount != 1_356 {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("canonical name count %d does not match required 1356", nameCount)
	}
	if err := validateSelectorCardinality(joined); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if err := promoteReviewedSelectorAliasCubes(joined); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}

	audit, err := applyCoverageManifest(joined, coverageManifestPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if err := validateRealProvenance(joined, audit); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}

	metadata := RegistryMetadata{
		Protocol:           protocol,
		CanonicalNames:     uint32(nameCount),
		CanonicalStates:    uint32(len(joined)),
		ValentineNames:     uint32(audit.ValentineNames),
		ValentineStates:    uint32(audit.ValentineStates),
		ValentineGapNames:  uint32(audit.GapNames),
		ValentineGapStates: uint32(audit.GapStates),
	}
	pmmpSHA, err := fileSHA256(pmmpPalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarineStateSHA, err := fileSHA256(prismarineStatesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarineShapeSHA, err := fileSHA256(prismarineShapesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	coverageManifestSHA, err := fileSHA256(coverageManifestPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	report := GenerationReport{
		Protocol:               protocol,
		CanonicalNames:         nameCount,
		CanonicalStates:        len(joined),
		ValentineAudit:         audit,
		PMMPPaletteSHA256:      pmmpSHA,
		PrismarineStateSHA256:  prismarineStateSHA,
		PrismarineShapeSHA256:  prismarineShapeSHA,
		CoverageManifestSHA256: coverageManifestSHA,
	}
	return joined, metadata, report, nil
}

func readProtocol(path string) (uint32, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	if len(data) > 1<<20 {
		return 0, fmt.Errorf("protocol metadata exceeds 1 MiB")
	}
	var source struct {
		Version struct {
			Major    int    `json:"major"`
			Minor    int    `json:"minor"`
			Patch    int    `json:"patch"`
			Protocol uint32 `json:"protocol_version"`
		} `json:"version"`
	}
	if err := json.Unmarshal(data, &source); err != nil {
		return 0, err
	}
	if source.Version.Major != 1 || source.Version.Minor != 26 || source.Version.Patch != 30 {
		return 0, fmt.Errorf("PMMP game version %d.%d.%d does not match 1.26.30", source.Version.Major, source.Version.Minor, source.Version.Patch)
	}
	return source.Version.Protocol, nil
}

type nbtState struct {
	Name       string         `nbt:"name"`
	Properties map[string]any `nbt:"states"`
	Version    int32          `nbt:"version"`
}

func readNBTStates(path string) ([]SourceState, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(data) > 16<<20 {
		return nil, fmt.Errorf("NBT palette is %d bytes, exceeding 16 MiB", len(data))
	}
	return decodeNBTStates(data)
}

func decodeNBTStates(data []byte) ([]SourceState, error) {
	reader := bytes.NewReader(data)
	decoder := nbt.NewDecoder(reader)
	states := make([]SourceState, 0, 16_913)
	for reader.Len() != 0 && len(states) <= maxRecordCount {
		var state nbtState
		err := decoder.Decode(&state)
		if err != nil {
			return nil, fmt.Errorf("decode state %d: %w", len(states), err)
		}
		properties, err := typedProperties(state.Properties)
		if err != nil {
			return nil, fmt.Errorf("state %d %s: %w", len(states), state.Name, err)
		}
		states = append(states, SourceState{Name: canonicalBlockName(state.Name), Properties: properties, Ordinal: uint32(len(states))})
	}
	if len(states) > maxRecordCount {
		return nil, fmt.Errorf("NBT palette exceeds %d records", maxRecordCount)
	}
	return states, nil
}

type prismarineStateValue struct {
	Type  string          `json:"type"`
	Value json.RawMessage `json:"value"`
}

type prismarineState struct {
	Name   string                          `json:"name"`
	States map[string]prismarineStateValue `json:"states"`
}

type prismarineCollisionShapes struct {
	Blocks map[string][]uint16    `json:"blocks"`
	Shapes map[string][][]float64 `json:"shapes"`
}

func readPrismarineStates(statesPath, shapesPath string) ([]SourceState, error) {
	statesData, err := os.ReadFile(statesPath)
	if err != nil {
		return nil, err
	}
	if len(statesData) > 16<<20 {
		return nil, fmt.Errorf("Prismarine blockStates.json exceeds 16 MiB")
	}
	var rawStates []prismarineState
	if err := json.Unmarshal(statesData, &rawStates); err != nil {
		return nil, err
	}
	if len(rawStates) > maxRecordCount {
		return nil, fmt.Errorf("Prismarine states exceed %d records", maxRecordCount)
	}
	shapesData, err := os.ReadFile(shapesPath)
	if err != nil {
		return nil, err
	}
	if len(shapesData) > 4<<20 {
		return nil, fmt.Errorf("Prismarine collision shapes exceed 4 MiB")
	}
	var shapes prismarineCollisionShapes
	if err := json.Unmarshal(shapesData, &shapes); err != nil {
		return nil, err
	}
	occurrence := make(map[string]int)
	totals := make(map[string]int)
	for _, raw := range rawStates {
		totals[canonicalBlockName(raw.Name)]++
	}
	result := make([]SourceState, 0, len(rawStates))
	for ordinal, raw := range rawStates {
		name := canonicalBlockName(raw.Name)
		properties := make([]StateProperty, 0, len(raw.States))
		for key, value := range raw.States {
			scalar, err := parsePrismarineScalar(value)
			if err != nil {
				return nil, fmt.Errorf("Prismarine state %d %s property %s: %w", ordinal, name, key, err)
			}
			properties = append(properties, StateProperty{Name: key, Value: scalar})
		}
		shapeIDs, ok := shapes.Blocks[strings.TrimPrefix(name, "minecraft:")]
		if !ok || len(shapeIDs) == 0 {
			return nil, fmt.Errorf("Prismarine state %d %s has no collision-shape mapping", ordinal, name)
		}
		index := occurrence[name]
		occurrence[name] = index + 1
		shapeID, err := collisionShapeID(shapeIDs, index, totals[name])
		if err != nil {
			return nil, fmt.Errorf("Prismarine state %d %s: %w", ordinal, name, err)
		}
		seed, err := collisionSeed(shapeID, shapes.Shapes)
		if err != nil {
			return nil, fmt.Errorf("Prismarine state %d %s: %w", ordinal, name, err)
		}
		result = append(result, SourceState{Name: name, Properties: properties, Ordinal: uint32(ordinal), CollisionSeed: seed})
	}
	return result, nil
}

func collisionShapeID(ids []uint16, occurrence, total int) (uint16, error) {
	if len(ids) == 0 {
		return 0, errors.New("collision shape mapping is empty")
	}
	if len(ids) == 1 {
		if occurrence < 0 || occurrence >= total {
			return 0, fmt.Errorf("collision occurrence %d is outside %d states", occurrence, total)
		}
		return ids[0], nil
	}
	if len(ids) != total {
		return 0, fmt.Errorf("collision variant cardinality %d does not match %d states", len(ids), total)
	}
	if occurrence < 0 || occurrence >= len(ids) {
		return 0, fmt.Errorf("collision occurrence %d is outside %d variants", occurrence, len(ids))
	}
	return ids[occurrence], nil
}

func parsePrismarineScalar(value prismarineStateValue) (TypedScalar, error) {
	switch value.Type {
	case "byte":
		var number int64
		if err := json.Unmarshal(value.Value, &number); err != nil || number < 0 || number > math.MaxUint8 {
			return TypedScalar{}, fmt.Errorf("invalid byte value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarByte, Byte: byte(number)}, nil
	case "int":
		var number int64
		if err := json.Unmarshal(value.Value, &number); err != nil || number < math.MinInt32 || number > math.MaxInt32 {
			return TypedScalar{}, fmt.Errorf("invalid int value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarInt, Int: int32(number)}, nil
	case "string":
		var text string
		if err := json.Unmarshal(value.Value, &text); err != nil {
			return TypedScalar{}, fmt.Errorf("invalid string value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarString, String: text}, nil
	default:
		return TypedScalar{}, fmt.Errorf("unsupported scalar type %q", value.Type)
	}
}

func collisionSeed(id uint16, shapes map[string][][]float64) (CollisionSeed, error) {
	raw, ok := shapes[strconv.FormatUint(uint64(id), 10)]
	if !ok {
		return CollisionSeed{}, fmt.Errorf("collision shape %d is missing", id)
	}
	if len(raw) > maxCollisionBoxesPerRecord {
		return CollisionSeed{}, fmt.Errorf("collision shape %d has %d boxes, exceeding %d", id, len(raw), maxCollisionBoxesPerRecord)
	}
	boxes := make([]CollisionBox, 0, len(raw))
	for index, coords := range raw {
		if len(coords) != 6 {
			return CollisionSeed{}, fmt.Errorf("collision shape %d box %d has %d coordinates", id, index, len(coords))
		}
		fixed := [6]int32{}
		for i, coordinate := range coords {
			scaled := math.Round(coordinate * collisionFixedScale)
			if math.IsNaN(coordinate) || math.IsInf(coordinate, 0) || scaled < math.MinInt32 || scaled > math.MaxInt32 || math.Abs(scaled-coordinate*collisionFixedScale) > 1e-6 {
				return CollisionSeed{}, fmt.Errorf("collision shape %d coordinate %g is not bounded exact 1/100000000 fixed point", id, coordinate)
			}
			fixed[i] = int32(scaled)
		}
		box := CollisionBox{MinX: fixed[0], MinY: fixed[1], MinZ: fixed[2], MaxX: fixed[3], MaxY: fixed[4], MaxZ: fixed[5]}
		if err := validateCollisionBox(box); err != nil {
			return CollisionSeed{}, fmt.Errorf("collision shape %d box %d: %w", id, index, err)
		}
		boxes = append(boxes, box)
	}
	return CollisionSeed{ShapeID: id, Confidence: CollisionConfidenceCollisionOnly, Boxes: boxes}, nil
}

func validateCollisionBox(box CollisionBox) error {
	if box.MinX > box.MaxX || box.MinY > box.MaxY || box.MinZ > box.MaxZ {
		return errors.New("collision bounds are inverted")
	}
	if box.MinX < collisionLocalHaloMin || box.MinY < collisionLocalHaloMin || box.MinZ < collisionLocalHaloMin ||
		box.MaxX > collisionLocalHaloMax || box.MaxY > collisionLocalHaloMax || box.MaxZ > collisionLocalHaloMax {
		return errors.New("collision bounds exceed the one-block query halo")
	}
	return nil
}

func readValentineBlockCount(path string) (int, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	if len(data) > 16<<20 {
		return 0, fmt.Errorf("Valentine blocks.rs exceeds 16 MiB")
	}
	marker := []byte("impl BlockDef for ")
	definitions := bytes.Count(data, marker)
	constantMarker := []byte("pub const BLOCK_COUNT: usize = ")
	start := bytes.Index(data, constantMarker)
	if start < 0 {
		return 0, fmt.Errorf("Valentine blocks.rs has no BLOCK_COUNT")
	}
	start += len(constantMarker)
	end := start
	for end < len(data) && data[end] >= '0' && data[end] <= '9' {
		end++
	}
	declared, err := strconv.Atoi(string(data[start:end]))
	if err != nil || declared != definitions {
		return 0, fmt.Errorf("Valentine BLOCK_COUNT %q does not match %d definitions", data[start:end], definitions)
	}
	return definitions, nil
}

func fileSHA256(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("hash source %s: %w", path, err)
	}
	digest := sha256.Sum256(data)
	return fmt.Sprintf("%x", digest), nil
}

func typedProperties(properties map[string]any) ([]StateProperty, error) {
	result := make([]StateProperty, 0, len(properties))
	for name, raw := range properties {
		var scalar TypedScalar
		switch value := raw.(type) {
		case bool:
			scalar.Kind = ScalarByte
			if value {
				scalar.Byte = 1
			}
		case uint8:
			scalar = TypedScalar{Kind: ScalarByte, Byte: value}
		case int8:
			scalar = TypedScalar{Kind: ScalarByte, Byte: byte(value)}
		case int32:
			scalar = TypedScalar{Kind: ScalarInt, Int: value}
		case string:
			scalar = TypedScalar{Kind: ScalarString, String: value}
		default:
			return nil, fmt.Errorf("property %s has unsupported scalar type %T", name, raw)
		}
		result = append(result, StateProperty{Name: name, Value: scalar})
	}
	return result, nil
}

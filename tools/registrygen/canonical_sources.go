package main

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
)

func canonicalBlockName(name string) string {
	if strings.ContainsRune(name, ':') {
		return name
	}
	return "minecraft:" + name
}

type canonicalScalar struct {
	Type  string `json:"type"`
	Value any    `json:"value"`
}

func canonicalTypedState(properties []StateProperty) ([]byte, error) {
	sorted := append([]StateProperty(nil), properties...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].Name < sorted[j].Name })
	canonical := make(map[string]canonicalScalar, len(sorted))
	for _, property := range sorted {
		if property.Name == "" {
			return nil, errors.New("state property name is empty")
		}
		if _, exists := canonical[property.Name]; exists {
			return nil, fmt.Errorf("duplicate state property %q", property.Name)
		}
		var scalar canonicalScalar
		switch property.Value.Kind {
		case ScalarByte:
			scalar = canonicalScalar{Type: "byte", Value: property.Value.Byte}
		case ScalarInt:
			scalar = canonicalScalar{Type: "int", Value: property.Value.Int}
		case ScalarString:
			scalar = canonicalScalar{Type: "string", Value: property.Value.String}
		default:
			return nil, fmt.Errorf("property %q has unknown scalar type %d", property.Name, property.Value.Kind)
		}
		canonical[property.Name] = scalar
	}
	return json.Marshal(canonical)
}

func canonicalRecordKey(name string, state []byte) string {
	return canonicalBlockName(name) + "\x00" + string(state)
}

func canonicalStateHash(key []byte) uint64 {
	digest := sha256.Sum256(key)
	return binary.LittleEndian.Uint64(digest[:8])
}

func canonicalSourceIndex(states []SourceState, source string, hasher func([]byte) uint64) (map[string]SourceState, error) {
	if len(states) > maxRecordCount {
		return nil, fmt.Errorf("%s has %d states, exceeding %d", source, len(states), maxRecordCount)
	}
	index := make(map[string]SourceState, len(states))
	hashes := make(map[uint64]string, len(states))
	for _, state := range states {
		state.Name = canonicalBlockName(state.Name)
		canonical, err := canonicalTypedState(state.Properties)
		if err != nil {
			return nil, fmt.Errorf("%s state %s: %w", source, state.Name, err)
		}
		key := canonicalRecordKey(state.Name, canonical)
		if _, exists := index[key]; exists {
			return nil, fmt.Errorf("duplicate canonical key in %s: %s %s", source, state.Name, canonical)
		}
		hash := hasher([]byte(key))
		if previous, exists := hashes[hash]; exists && previous != key {
			return nil, fmt.Errorf("canonical state hash collision in %s between %q and %q", source, previous, key)
		}
		hashes[hash] = key
		index[key] = state
	}
	return index, nil
}

func indexNames(states []SourceState) map[string]int {
	result := make(map[string]int)
	for _, state := range states {
		result[canonicalBlockName(state.Name)]++
	}
	return result
}

func joinSources(pmmp, dragonfly, prismarine []SourceState, hasher func([]byte) uint64) ([]Record, error) {
	pmmpIndex, err := canonicalSourceIndex(pmmp, "PMMP", hasher)
	if err != nil {
		return nil, err
	}
	dragonflyIndex, err := canonicalSourceIndex(dragonfly, "Dragonfly", hasher)
	if err != nil {
		return nil, err
	}
	prismarineIndex, err := canonicalSourceIndex(prismarine, "Prismarine", hasher)
	if err != nil {
		return nil, err
	}
	dragonflyNames := indexNames(dragonfly)
	prismarineNames := indexNames(prismarine)
	for key, source := range pmmpIndex {
		if _, ok := dragonflyIndex[key]; !ok {
			if dragonflyNames[source.Name] != 0 {
				return nil, fmt.Errorf("typed state mismatch for %s: PMMP state %q missing from Dragonfly", source.Name, key)
			}
			return nil, fmt.Errorf("canonical state %q missing from Dragonfly", key)
		}
		if _, ok := prismarineIndex[key]; !ok {
			if prismarineNames[source.Name] != 0 {
				return nil, fmt.Errorf("typed state mismatch for %s: PMMP state %q missing from Prismarine", source.Name, key)
			}
			return nil, fmt.Errorf("canonical state %q missing from Prismarine", key)
		}
	}
	for key := range dragonflyIndex {
		if _, ok := pmmpIndex[key]; !ok {
			return nil, fmt.Errorf("canonical state %q extra in Dragonfly", key)
		}
	}
	for key := range prismarineIndex {
		if _, ok := pmmpIndex[key]; !ok {
			return nil, fmt.Errorf("canonical state %q extra in Prismarine", key)
		}
	}

	keys := make([]string, 0, len(pmmpIndex))
	for key := range pmmpIndex {
		keys = append(keys, key)
	}
	sort.Slice(keys, func(i, j int) bool {
		left, right := dragonflyIndex[keys[i]], dragonflyIndex[keys[j]]
		if left.Ordinal != right.Ordinal {
			return left.Ordinal < right.Ordinal
		}
		return keys[i] < keys[j]
	})
	result := make([]Record, 0, len(keys))
	for _, key := range keys {
		pmmpState := pmmpIndex[key]
		dragonflyState := dragonflyIndex[key]
		prismarineState := prismarineIndex[key]
		record, err := classifyRecord(pmmpState)
		if err != nil {
			return nil, err
		}
		record.SequentialID = dragonflyState.Ordinal
		record.NetworkHash = dragonflyState.NetworkHash
		record.Flags = dragonflyState.Flags
		record.CollisionSeed = prismarineState.CollisionSeed
		record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		finalizeGeometryFacts(&record)
		result = append(result, record)
	}
	return result, nil
}

func collisionSeedIsUnit(seed CollisionSeed) bool {
	if seed.Confidence == CollisionConfidenceNone || len(seed.Boxes) != 1 {
		return false
	}
	box := seed.Boxes[0]
	return box == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func chiseledBookshelfCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 1 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func finalizeGeometryFacts(record *Record) {
	if isBeeHousingName(record.Name) {
		if beeHousingCollisionIsExact(record.CollisionSeed) {
			record.Flags = flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0x3f
		} else {
			record.Flags &^= flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0
		}
		return
	}
	if record.ModelFamily == ModelFamilyChiseledBookshelf {
		if chiseledBookshelfCollisionIsExact(record.CollisionSeed) {
			record.Flags = flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0x3f
		} else {
			record.Flags &^= flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0
		}
		return
	}
	if (record.ModelFamily == ModelFamilyStatue || record.ModelFamily == ModelFamilyCuboid) && record.CollisionSeed.Confidence != CollisionConfidenceNone && !collisionSeedIsUnit(record.CollisionSeed) {
		record.Flags &^= flagCubeGeometry | flagOccludesFullFace
		record.FaceCoverage = 0
		return
	}
	if record.Flags&flagLeafModel != 0 {
		record.ModelFamily = ModelFamilyLeaves
	} else if record.Flags&flagCubeGeometry != 0 && record.ModelFamily == ModelFamilyUnknown {
		record.ModelFamily = ModelFamilyCube
	}
	if record.Flags&flagOccludesFullFace != 0 || (record.ModelFamily == ModelFamilyCube && collisionSeedIsUnit(record.CollisionSeed)) {
		record.FaceCoverage = 0x3f
	}
}

func auditValentineSubset(canonical, valentine []SourceState) (ValentineAudit, error) {
	canonicalIndex, err := canonicalSourceIndex(canonical, "canonical", canonicalStateHash)
	if err != nil {
		return ValentineAudit{}, err
	}
	valentineIndex, err := canonicalSourceIndex(valentine, "Valentine", canonicalStateHash)
	if err != nil {
		return ValentineAudit{}, err
	}
	canonicalNames := indexNames(canonical)
	valentineNames := indexNames(valentine)
	for key, state := range valentineIndex {
		if _, ok := canonicalIndex[key]; !ok {
			if canonicalNames[state.Name] != 0 {
				return ValentineAudit{}, fmt.Errorf("Valentine typed state %q does not match canonical source", key)
			}
			return ValentineAudit{}, fmt.Errorf("Valentine state %q is outside canonical source", key)
		}
	}
	expectedOrder := make([]string, 0, len(valentineIndex))
	for _, state := range canonical {
		encoded, err := canonicalTypedState(state.Properties)
		if err != nil {
			return ValentineAudit{}, err
		}
		key := canonicalRecordKey(state.Name, encoded)
		if _, ok := valentineIndex[key]; ok {
			expectedOrder = append(expectedOrder, key)
		}
	}
	actualOrder := make([]string, 0, len(valentine))
	for _, state := range valentine {
		encoded, err := canonicalTypedState(state.Properties)
		if err != nil {
			return ValentineAudit{}, err
		}
		actualOrder = append(actualOrder, canonicalRecordKey(state.Name, encoded))
	}
	if len(expectedOrder) != len(actualOrder) {
		return ValentineAudit{}, fmt.Errorf("Valentine overlap order cardinality %d does not match %d", len(actualOrder), len(expectedOrder))
	}
	for index := range expectedOrder {
		if expectedOrder[index] != actualOrder[index] {
			return ValentineAudit{}, fmt.Errorf("Valentine overlap order differs at %d: got %q want %q", index, actualOrder[index], expectedOrder[index])
		}
	}
	missingNames := make([]string, 0)
	for name := range canonicalNames {
		if valentineNames[name] == 0 {
			missingNames = append(missingNames, name)
		}
	}
	sort.Strings(missingNames)
	missingStates := make([]string, 0, len(canonicalIndex)-len(valentineIndex))
	for key := range canonicalIndex {
		if _, ok := valentineIndex[key]; !ok {
			missingStates = append(missingStates, key)
		}
	}
	sort.Strings(missingStates)
	return ValentineAudit{
		CanonicalNames:  len(canonicalNames),
		CanonicalStates: len(canonicalIndex),
		ValentineNames:  len(valentineNames),
		ValentineStates: len(valentineIndex),
		GapNames:        len(missingNames),
		GapStates:       len(missingStates),
		Joined:          len(valentineIndex),
		Missing:         len(missingStates),
		Extra:           0,
		Mismatched:      0,
		MissingNames:    missingNames,
		MissingStates:   missingStates,
	}, nil
}

func validateRealProvenance(records []Record, audit ValentineAudit) error {
	if len(records) != audit.CanonicalStates {
		return fmt.Errorf("provenance record count %d does not match canonical %d", len(records), audit.CanonicalStates)
	}
	canonicalBits := ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
	valentineCount := 0
	for index, record := range records {
		if record.Provenance&canonicalBits != canonicalBits {
			return fmt.Errorf("record %d has incomplete canonical provenance %#x", index, record.Provenance)
		}
		if record.Provenance&ProvenanceValentine != 0 {
			valentineCount++
		}
	}
	if valentineCount != audit.ValentineStates || valentineCount != audit.Joined {
		return fmt.Errorf("Valentine provenance count %d does not match audit %d/%d", valentineCount, audit.ValentineStates, audit.Joined)
	}
	return nil
}

func uniqueNameCount(states []SourceState) int {
	return len(indexNames(states))
}

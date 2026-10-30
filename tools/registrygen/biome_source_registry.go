package main

import (
	"encoding/binary"
	"errors"
	"fmt"
	"math"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
)

func collectBiomes(biomes []world.Biome) ([]BiomeRecord, error) {
	if len(biomes) > maxBiomeRecordCount {
		return nil, fmt.Errorf("too many biome records: %d exceeds %d", len(biomes), maxBiomeRecordCount)
	}
	records := make([]BiomeRecord, 0, len(biomes))
	for _, biome := range biomes {
		if biome == nil {
			return nil, errors.New("biome registry contains nil biome")
		}
		id := biome.EncodeBiome()
		if id < 0 || id > math.MaxUint16 {
			return nil, fmt.Errorf("biome ID %d is outside 0..%d", id, uint16(math.MaxUint16))
		}
		name := canonicalBiomeName(biome.String())
		records = append(records, BiomeRecord{ID: uint32(id), Name: name})
	}
	return records, nil
}

func canonicalBiomeName(name string) string {
	if strings.ContainsRune(name, ':') || name == "" {
		return name
	}
	return "minecraft:" + name
}

func encodeBiomeRegistry(records []BiomeRecord) ([]byte, error) {
	if len(records) > maxBiomeRecordCount {
		return nil, fmt.Errorf("too many biome records: %d exceeds %d", len(records), maxBiomeRecordCount)
	}

	sorted := append([]BiomeRecord(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].ID < sorted[j].ID })
	seenIDs := make(map[uint32]struct{}, len(sorted))
	seenNames := make(map[string]struct{}, len(sorted))
	for _, record := range sorted {
		if record.ID > math.MaxUint16 {
			return nil, fmt.Errorf("biome ID %d is outside 0..%d", record.ID, uint16(math.MaxUint16))
		}
		if _, exists := seenIDs[record.ID]; exists {
			return nil, fmt.Errorf("duplicate biome ID: %d", record.ID)
		}
		seenIDs[record.ID] = struct{}{}
		if record.Name == "" {
			return nil, fmt.Errorf("biome name is empty for ID %d", record.ID)
		}
		if len(record.Name) > maxBiomeNameBytes {
			return nil, fmt.Errorf("biome name too long for ID %d: %d bytes exceeds %d", record.ID, len(record.Name), maxBiomeNameBytes)
		}
		if _, exists := seenNames[record.Name]; exists {
			return nil, fmt.Errorf("duplicate biome name: %s", record.Name)
		}
		seenNames[record.Name] = struct{}{}
	}

	encoded := make([]byte, 0, len(biomeRegistryHeader)+4)
	encoded = append(encoded, biomeRegistryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(sorted)))
	for _, record := range sorted {
		encoded = binary.LittleEndian.AppendUint32(encoded, record.ID)
		encoded = binary.LittleEndian.AppendUint16(encoded, uint16(len(record.Name)))
		encoded = append(encoded, record.Name...)
	}
	return encoded, nil
}

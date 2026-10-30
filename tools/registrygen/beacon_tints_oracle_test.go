package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strings"
)

var beaconTints, beaconTintError = loadBeaconTints(".")

// loadBeaconTints validates the eligibility table found above start, before tests or commands change directories.
func loadBeaconTints(start string) (map[string]BeaconTint, error) {
	data, err := readAssetData(start, "beacon-tints.json")
	if err != nil {
		return nil, err
	}
	var table struct {
		Schema uint32 `json:"schema"`
		Colors []struct {
			Name string     `json:"name"`
			Tint BeaconTint `json:"tint"`
		} `json:"colors"`
		Variants []string `json:"variants"`
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&table); err != nil {
		return nil, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return nil, fmt.Errorf("beacon tint table contains trailing data")
	}
	if table.Schema != 1 || len(table.Colors) == 0 || len(table.Variants) == 0 {
		return nil, fmt.Errorf("beacon tint table is empty or has an unsupported version")
	}
	entries := make(map[string]BeaconTint)
	for _, color := range table.Colors {
		if color.Name == "" || color.Tint < BeaconTintWhite || color.Tint > maxBeaconTint {
			return nil, fmt.Errorf("beacon tint table has an invalid color")
		}
		for _, variant := range table.Variants {
			prefix, suffix := "minecraft:", variant
			if strings.HasPrefix(variant, "hard_") {
				prefix, suffix = "minecraft:hard_", strings.TrimPrefix(variant, "hard_")
			}
			name := prefix + color.Name + "_" + suffix
			if _, exists := entries[name]; exists {
				return nil, fmt.Errorf("duplicate beacon tint %s", name)
			}
			entries[name] = color.Tint
		}
	}
	return entries, nil
}

// applyBeaconTints binds every canonical state to a tint or a known non-tinting disposition.
func applyBeaconTints(records, source []Record) error {
	if beaconTintError != nil {
		return beaconTintError
	}
	identities := make(map[uint32]string, len(source))
	for _, record := range source {
		identities[record.NetworkHash] = record.Name
	}
	for index := range records {
		name, known := identities[records[index].NetworkHash]
		if !known {
			return fmt.Errorf("beacon disposition has no source identity for hash %d", records[index].NetworkHash)
		}
		tint, found := beaconTints[name]
		if !found {
			tint = BeaconTintNoTinting
		}
		records[index].BeaconTint = tint
	}
	return nil
}

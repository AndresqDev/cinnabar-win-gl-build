package main

import (
	"encoding/json"
	"fmt"
)

func promoteReviewedSelectorAliasCubes(records []Record) error {
	type product struct {
		base          uint32
		cardinality   int
		hasDeprecated bool
		tnt           bool
	}
	products := map[string]product{
		"minecraft:hay_block":             {base: 2907, cardinality: 12, hasDeprecated: true},
		"minecraft:bone_block":            {base: 6465, cardinality: 12, hasDeprecated: true},
		"minecraft:quartz_block":          {base: 5442, cardinality: 3},
		"minecraft:smooth_quartz":         {base: 7081, cardinality: 3},
		"minecraft:chiseled_quartz_block": {base: 14685, cardinality: 3},
		"minecraft:purpur_block":          {base: 15344, cardinality: 3},
		"minecraft:tnt":                   {base: 13112, cardinality: 2, tnt: true},
	}
	groups := make(map[string][]int, len(products))
	for index := range records {
		if _, reviewed := products[records[index].Name]; reviewed {
			groups[records[index].Name] = append(groups[records[index].Name], index)
		}
	}
	if len(groups) == 0 {
		return nil
	}
	promote := make([]int, 0, 27)
	for name, spec := range products {
		indexes := groups[name]
		if len(indexes) != spec.cardinality {
			return fmt.Errorf("%s selector cardinality is %d, want %d", name, len(indexes), spec.cardinality)
		}
		seen := make([]bool, spec.cardinality)
		for _, index := range indexes {
			record := &records[index]
			if record.ModelFamily != ModelFamilyCube || record.ContributorRole != ContributorPrimary {
				return fmt.Errorf("%s state %d has invalid family or role", name, record.SequentialID)
			}
			if record.FaceCoverage != 0x3f || !selectorAliasCubeCollisionIsExact(record.CollisionSeed) {
				return fmt.Errorf("%s state %d lacks exact unit cube evidence", name, record.SequentialID)
			}
			var offset uint32
			var sourceSolid bool
			if spec.tnt {
				var state map[string]json.RawMessage
				if err := json.Unmarshal(record.StateJSON, &state); err != nil || len(state) != 1 {
					return fmt.Errorf("%s state %d has invalid typed selector", name, record.SequentialID)
				}
				exploded, ok := exactCanonicalByte(state["explode_bit"])
				if !ok || record.ModelState.Mask != 0 {
					return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
				}
				offset = uint32(exploded)
				sourceSolid = exploded == 0
			} else {
				axisIndex, deprecated, ok := selectorAliasAxisState(record.StateJSON, spec.hasDeprecated)
				orientation, hasOrientation := record.ModelState.Get(ModelStateOrientation)
				wantOrientation := [3]uint32{1, 0, 2}[axisIndex]
				if !ok || !hasOrientation || orientation != wantOrientation || record.ModelState.Mask != uint8(1<<(ModelStateOrientation-1)) {
					return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
				}
				stride := uint32(1)
				if spec.hasDeprecated {
					stride = 4
				}
				offset = axisIndex*stride + deprecated
				if spec.hasDeprecated {
					sourceSolid = deprecated == 0
				} else {
					sourceSolid = axisIndex == 0
				}
			}
			if offset >= uint32(spec.cardinality) || record.SequentialID != spec.base+offset {
				return fmt.Errorf("%s state %d does not match canonical ID formula", name, record.SequentialID)
			}
			if seen[offset] {
				return fmt.Errorf("%s has duplicate selector offset %d", name, offset)
			}
			seen[offset] = true
			wantFlags := uint8(0)
			if sourceSolid {
				wantFlags = flagCubeGeometry | flagOccludesFullFace
			}
			if record.Flags != wantFlags {
				return fmt.Errorf("%s state %d has unexpected pre-promotion flags %#x", name, record.SequentialID, record.Flags)
			}
			if !sourceSolid {
				promote = append(promote, index)
			}
		}
		for offset, present := range seen {
			if !present {
				return fmt.Errorf("%s selector product is missing offset %d", name, offset)
			}
		}
	}
	for _, index := range promote {
		records[index].Flags = flagCubeGeometry | flagOccludesFullFace
		records[index].FaceCoverage = 0x3f
	}
	return nil
}

func validateSelectorCardinality(records []Record) error {
	if err := validateShelfInventory(records); err != nil {
		return err
	}
	groups := make(map[string][]Record)
	for _, record := range records {
		groups[record.Name] = append(groups[record.Name], record)
	}
	for name, group := range groups {
		if isBeeHousingName(name) {
			if err := validateBeeHousingProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:chiseled_bookshelf" {
			if err := validateChiseledBookshelfProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:resin_clump" {
			if err := validateResinClumpProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:cactus" {
			if err := validateCactusProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:cake" {
			if err := validateCakeProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:farmland" {
			if err := validateFarmlandProduct(group); err != nil {
				return err
			}
		}
		values := make(map[string]map[string]struct{})
		for _, record := range group {
			var state map[string]canonicalScalar
			if err := json.Unmarshal(record.StateJSON, &state); err != nil {
				return fmt.Errorf("selector %s has invalid canonical state: %w", name, err)
			}
			for property, value := range state {
				encoded, _ := json.Marshal(value)
				if values[property] == nil {
					values[property] = make(map[string]struct{})
				}
				values[property][string(encoded)] = struct{}{}
			}
		}
		expected := 1
		for _, distinct := range values {
			if expected > maxRecordCount/len(distinct) {
				return fmt.Errorf("selector cardinality for %s overflows bound", name)
			}
			expected *= len(distinct)
		}
		if expected != len(group) {
			return fmt.Errorf("selector cardinality for %s is %d states but typed product is %d", name, len(group), expected)
		}
	}
	return nil
}

func beeHousingCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 1 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func validateShelfInventory(records []Record) error {
	candidates := make([]Record, 0, 384)
	for _, record := range records {
		if isShelfCandidateName(record.Name) {
			candidates = append(candidates, record)
		}
	}
	if len(candidates) == 0 {
		return nil
	}
	if len(candidates) != 384 {
		return fmt.Errorf("shelf inventory has %d records, want exactly 384", len(candidates))
	}
	wantNames := []string{
		"minecraft:acacia_shelf",
		"minecraft:bamboo_shelf",
		"minecraft:birch_shelf",
		"minecraft:cherry_shelf",
		"minecraft:crimson_shelf",
		"minecraft:dark_oak_shelf",
		"minecraft:jungle_shelf",
		"minecraft:mangrove_shelf",
		"minecraft:oak_shelf",
		"minecraft:pale_oak_shelf",
		"minecraft:spruce_shelf",
		"minecraft:warped_shelf",
	}
	groups := make(map[string][]Record, len(wantNames))
	for _, record := range candidates {
		if !isShelfName(record.Name) {
			return fmt.Errorf("shelf inventory contains unexpected family %q", record.Name)
		}
		groups[record.Name] = append(groups[record.Name], record)
	}
	if len(groups) != len(wantNames) {
		return fmt.Errorf("shelf inventory has %d families, want exactly %d", len(groups), len(wantNames))
	}
	for _, name := range wantNames {
		group := groups[name]
		if len(group) != 32 {
			return fmt.Errorf("shelf inventory family %s has %d records, want exactly 32", name, len(group))
		}
	}
	for _, name := range wantNames {
		if err := validateShelfProduct(groups[name]); err != nil {
			return err
		}
	}
	return nil
}

func validateBeeHousingProduct(records []Record) error {
	if len(records) != 24 {
		return fmt.Errorf("bee housing selector cardinality is %d, want 24", len(records))
	}
	name := records[0].Name
	base := uint32(0)
	switch name {
	case "minecraft:bee_nest":
		base = 10_395
	case "minecraft:beehive":
		base = 12_495
	default:
		return fmt.Errorf("unsupported bee housing name %q", name)
	}
	seen := [24]bool{}
	wantMask := uint8(1<<(ModelStateOrientation-1) | 1<<(ModelStateGrowth-1))
	for _, record := range records {
		if record.Name != name || record.ModelFamily != ModelFamilyCube || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("%s state %d has invalid family or role", name, record.SequentialID)
		}
		if record.Flags != flagCubeGeometry|flagOccludesFullFace || record.FaceCoverage != 0x3f || !beeHousingCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("%s state %d lacks exact solid unit geometry evidence", name, record.SequentialID)
		}
		canonicalDirection, canonicalHoney, hasCanonical := beeHousingCanonicalSelectors(record.StateJSON)
		direction, hasDirection := record.ModelState.Get(ModelStateOrientation)
		honey, hasHoney := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasDirection || !hasHoney || canonicalDirection != direction || canonicalHoney != honey || direction > 3 || honey > 5 || record.ModelState.Mask != wantMask {
			return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
		}
		offset := honey*4 + direction
		if record.SequentialID != base+offset {
			return fmt.Errorf("%s state %d does not match canonical ID formula", name, record.SequentialID)
		}
		if seen[offset] {
			return fmt.Errorf("%s has duplicate selector offset %d", name, offset)
		}
		seen[offset] = true
	}
	for offset, present := range seen {
		if !present {
			return fmt.Errorf("%s selector product is missing offset %d", name, offset)
		}
	}
	return nil
}

func shelfBaseID(name string) (uint32, bool) {
	switch name {
	case "minecraft:acacia_shelf":
		return 383, true
	case "minecraft:bamboo_shelf":
		return 6513, true
	case "minecraft:birch_shelf":
		return 302, true
	case "minecraft:cherry_shelf":
		return 14007, true
	case "minecraft:crimson_shelf":
		return 13882, true
	case "minecraft:dark_oak_shelf":
		return 9131, true
	case "minecraft:jungle_shelf":
		return 6045, true
	case "minecraft:mangrove_shelf":
		return 5280, true
	case "minecraft:oak_shelf":
		return 6897, true
	case "minecraft:pale_oak_shelf":
		return 11080, true
	case "minecraft:spruce_shelf":
		return 5162, true
	case "minecraft:warped_shelf":
		return 5313, true
	default:
		return 0, false
	}
}

func shelfCollisionIsExact(seed CollisionSeed, direction uint32) bool {
	if seed.Confidence != CollisionConfidenceCollisionOnly || len(seed.Boxes) != 1 {
		return false
	}
	var shapeID uint16
	var box CollisionBox
	switch direction {
	case 0:
		shapeID = 18
		box = CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 31_250_000}
	case 1:
		shapeID = 19
		box = CollisionBox{MinX: 68_750_000, MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}
	case 2:
		shapeID = 20
		box = CollisionBox{MinZ: 68_750_000, MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}
	case 3:
		shapeID = 21
		box = CollisionBox{MaxX: 31_250_000, MaxY: 100_000_000, MaxZ: 100_000_000}
	default:
		return false
	}
	return seed.ShapeID == shapeID && seed.Boxes[0] == box
}

func validateShelfProduct(records []Record) error {
	if len(records) != 32 {
		return fmt.Errorf("shelf selector cardinality is %d, want 32", len(records))
	}
	name := records[0].Name
	base, supported := shelfBaseID(name)
	if !supported {
		return fmt.Errorf("unsupported shelf name %q", name)
	}
	seen := [32]bool{}
	wantMask := uint8(1<<(ModelStateOrientation-1) | 1<<(ModelStateGrowth-1) | 1<<(ModelStateFlags-1))
	for _, record := range records {
		if record.Name != name || record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("%s state %d has invalid family or role", name, record.SequentialID)
		}
		direction, powered, shelfType, hasCanonical := shelfCanonicalSelectors(record.StateJSON)
		projectedDirection, hasDirection := record.ModelState.Get(ModelStateOrientation)
		projectedType, hasType := record.ModelState.Get(ModelStateGrowth)
		projectedFlags, hasFlags := record.ModelState.Get(ModelStateFlags)
		if !hasCanonical || !hasDirection || !hasType || !hasFlags ||
			projectedDirection != direction || projectedType != shelfType ||
			projectedFlags != powered*modelFlagPowered || record.ModelState.Mask != wantMask {
			return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !shelfCollisionIsExact(record.CollisionSeed, direction) {
			return fmt.Errorf("%s state %d lacks exact directional shelf geometry evidence", name, record.SequentialID)
		}
		offset := direction*8 + powered*4 + shelfType
		if record.SequentialID != base+offset {
			return fmt.Errorf("%s state %d does not match canonical ID formula", name, record.SequentialID)
		}
		if seen[offset] {
			return fmt.Errorf("%s has duplicate selector offset %d", name, offset)
		}
		seen[offset] = true
	}
	for offset, present := range seen {
		if !present {
			return fmt.Errorf("%s selector product is missing offset %d", name, offset)
		}
	}
	return nil
}

func validateChiseledBookshelfProduct(records []Record) error {
	if len(records) != 256 {
		return fmt.Errorf("chiseled_bookshelf selector cardinality is %d, want 256", len(records))
	}
	seen := make(map[[2]uint32]struct{}, 256)
	for _, record := range records {
		if record.ModelFamily != ModelFamilyChiseledBookshelf || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("chiseled_bookshelf state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != flagCubeGeometry|flagOccludesFullFace || !chiseledBookshelfCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("chiseled_bookshelf state %d lacks exact solid unit geometry evidence", record.SequentialID)
		}
		canonicalBooks, canonicalDirection, hasCanonicalState := chiseledBookshelfCanonicalSelectors(record.StateJSON)
		books, hasBooks := record.ModelState.Get(ModelStateConnections)
		direction, hasDirection := record.ModelState.Get(ModelStateOrientation)
		if !hasCanonicalState || canonicalBooks != books || canonicalDirection != direction || !hasBooks || !hasDirection || record.ModelState.Mask != uint8(1<<(ModelStateOrientation-1)|1<<(ModelStateConnections-1)) || books > 63 || direction > 3 {
			return fmt.Errorf("chiseled_bookshelf state %d has invalid typed selector projection", record.SequentialID)
		}
		if record.SequentialID != 1605+books*4+direction {
			return fmt.Errorf("chiseled_bookshelf state %d does not match canonical ID formula", record.SequentialID)
		}
		key := [2]uint32{books, direction}
		if _, exists := seen[key]; exists {
			return fmt.Errorf("chiseled_bookshelf duplicate selector %v", key)
		}
		seen[key] = struct{}{}
	}
	return nil
}

func resinClumpCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 0 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 0
}

func validateResinClumpProduct(records []Record) error {
	if len(records) != 64 {
		return fmt.Errorf("resin_clump selector cardinality is %d, want 64", len(records))
	}
	seen := [64]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyResinClump || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("resin_clump state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !resinClumpCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("resin_clump state %d has invalid empty geometry evidence", record.SequentialID)
		}
		canonical, hasCanonical := resinClumpCanonicalSelector(record.StateJSON)
		connections, hasConnections := record.ModelState.Get(ModelStateConnections)
		if !hasCanonical || !hasConnections || canonical != connections || connections > 63 || record.ModelState.Mask != uint8(1<<(ModelStateConnections-1)) {
			return fmt.Errorf("resin_clump state %d has invalid typed selector projection", record.SequentialID)
		}
		if record.SequentialID != 2930+connections {
			return fmt.Errorf("resin_clump state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[connections] {
			return fmt.Errorf("resin_clump duplicate selector %d", connections)
		}
		seen[connections] = true
	}
	for mask, present := range seen {
		if !present {
			return fmt.Errorf("resin_clump selector product is missing mask %d", mask)
		}
	}
	return nil
}

func cactusCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 84 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{
			MinX: 6_250_000, MaxX: 93_750_000,
			MinY: 0, MaxY: 100_000_000,
			MinZ: 6_250_000, MaxZ: 93_750_000,
		})
}

func validateCactusProduct(records []Record) error {
	if len(records) != 16 {
		return fmt.Errorf("cactus selector cardinality is %d, want 16", len(records))
	}
	seen := [16]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("cactus state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !cactusCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("cactus state %d has invalid inset geometry evidence", record.SequentialID)
		}
		canonical, hasCanonical := cactusCanonicalAge(record.StateJSON)
		age, hasAge := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasAge || canonical != age || age > 15 || record.ModelState.Mask != uint8(1<<(ModelStateGrowth-1)) {
			return fmt.Errorf("cactus state %d has invalid typed age projection", record.SequentialID)
		}
		if record.SequentialID != 13606+age {
			return fmt.Errorf("cactus state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[age] {
			return fmt.Errorf("cactus duplicate age %d", age)
		}
		seen[age] = true
	}
	for age, present := range seen {
		if !present {
			return fmt.Errorf("cactus selector product is missing age %d", age)
		}
	}
	return nil
}

func cakeCollisionIsExact(seed CollisionSeed, bite uint32) bool {
	if bite > 6 || seed.ShapeID != uint16(89+bite) ||
		seed.Confidence != CollisionConfidenceCollisionOnly || len(seed.Boxes) != 1 {
		return false
	}
	wantMinX := [...]int32{6_250_000, 18_750_000, 31_250_000, 43_750_000, 56_250_000, 68_750_000, 81_250_000}
	return seed.Boxes[0] == (CollisionBox{
		MinX: wantMinX[bite], MaxX: 93_750_000,
		MinY: 0, MaxY: 50_000_000,
		MinZ: 6_250_000, MaxZ: 93_750_000,
	})
}

func validateCakeProduct(records []Record) error {
	if len(records) != 7 {
		return fmt.Errorf("cake selector cardinality is %d, want 7", len(records))
	}
	seen := [7]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("cake state %d has invalid family or role", record.SequentialID)
		}
		canonical, hasCanonical := cakeCanonicalBite(record.StateJSON)
		bite, hasBite := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasBite || canonical != bite || bite > 6 ||
			record.ModelState.Mask != uint8(1<<(ModelStateGrowth-1)) {
			return fmt.Errorf("cake state %d has invalid typed bite projection", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !cakeCollisionIsExact(record.CollisionSeed, bite) {
			return fmt.Errorf("cake state %d has invalid exact geometry evidence", record.SequentialID)
		}
		if record.SequentialID != 14055+bite {
			return fmt.Errorf("cake state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[bite] {
			return fmt.Errorf("cake duplicate bite %d", bite)
		}
		seen[bite] = true
	}
	for bite, present := range seen {
		if !present {
			return fmt.Errorf("cake selector product is missing bite %d", bite)
		}
	}
	return nil
}

func farmlandCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 43 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{
			MinX: 0, MaxX: 100_000_000,
			MinY: 0, MaxY: 93_750_000,
			MinZ: 0, MaxZ: 100_000_000,
		})
}

func validateFarmlandProduct(records []Record) error {
	if len(records) != 8 {
		return fmt.Errorf("farmland selector cardinality is %d, want 8", len(records))
	}
	seen := [8]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("farmland state %d has invalid family or role", record.SequentialID)
		}
		canonical, hasCanonical := farmlandCanonicalMoisture(record.StateJSON)
		amount, hasAmount := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasAmount || canonical != amount || amount > 7 ||
			record.ModelState.Mask != uint8(1<<(ModelStateGrowth-1)) {
			return fmt.Errorf("farmland state %d has invalid typed moisture projection", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !farmlandCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("farmland state %d has invalid exact geometry evidence", record.SequentialID)
		}
		if record.SequentialID != 6122+amount {
			return fmt.Errorf("farmland state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[amount] {
			return fmt.Errorf("farmland duplicate moisture %d", amount)
		}
		seen[amount] = true
	}
	for amount, present := range seen {
		if !present {
			return fmt.Errorf("farmland selector product is missing amount %d", amount)
		}
	}
	return nil
}

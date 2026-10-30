It 'accepts an exact frustum subset without comparing key hashes to generation hashes' {
    $record = New-SyntheticPublication -RequiredColumns 2 -LoadedColumns 2 `
        -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2
    $record.presentation.publisher_disk.generation_manifest_hash = 'aaaaaaaaaaaaaaaa'
    $record.presentation.allocation.generation_manifest_hash = 'aaaaaaaaaaaaaaaa'
    $record.presentation.resident.generation_manifest_hash = 'bbbbbbbbbbbbbbbb'
    foreach ($name in @('visible', 'submitted', 'gpu_presented')) {
        $record.presentation.$name.entry_count = 1
        $record.presentation.$name.generation_manifest_hash = 'cccccccccccccccc'
    }

    (Get-PublicationFirstStalledStage -PublicationRecord $record -WorldReadyObserved:$true) |
        Should Be 'none'
}

It 'rejects manifest hashes mislabeled as a cross-stage domain' {
    $record = New-SyntheticPublication -RequiredColumns 1 -LoadedColumns 1 `
        -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1
    $record.presentation.visible.manifest_domain = 'key_generation'

    { Assert-PublicationRecord -Record $record -ExpectedPresentMode Fifo } |
        Should Throw
}

It 'rejects a frustum set that is not a proved resident subset' {
    $record = New-SyntheticPublication -RequiredColumns 1 -LoadedColumns 1 `
        -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1
    $record.presentation.visible_subset_of_resident = $false

    (Get-PublicationFirstStalledStage -PublicationRecord $record -WorldReadyObserved:$true) |
        Should Be 'extraction'
}

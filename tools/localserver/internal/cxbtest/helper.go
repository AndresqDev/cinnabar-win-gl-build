// Package cxbtest builds the existing offline verifier for Go integration tests.
package cxbtest

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os/exec"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
)

var buildOnce = sync.OnceValues(build)

// Binary returns the Cargo-reported CLI executable, building it once for this test process.
func Binary(t *testing.T) string {
	t.Helper()
	path, err := buildOnce()
	if err != nil {
		t.Fatal(err)
	}
	return path
}

// build locates the checkout from this source file and reads Cargo's executable artifact.
func build() (string, error) {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		return "", fmt.Errorf("cannot locate bundle verifier source")
	}
	root := filepath.Clean(filepath.Join(filepath.Dir(source), "..", "..", "..", ".."))
	cmd := exec.Command("cargo", "build", "-p", "cinnabar-cxb", "--locked", "--message-format=json-render-diagnostics")
	cmd.Dir = root
	var diagnostic bytes.Buffer
	cmd.Stderr = &diagnostic
	out, err := cmd.Output()
	if err != nil {
		return "", fmt.Errorf("building cinnabar-cxb: %w: %s", err, diagnostic.String())
	}
	decoder := json.NewDecoder(bytes.NewReader(out))
	for decoder.More() {
		var artifact struct {
			Reason     string
			Target     struct{ Name string }
			Executable string
		}
		if err := decoder.Decode(&artifact); err != nil {
			return "", err
		}
		if artifact.Reason == "compiler-artifact" && artifact.Target.Name == "cinnabar-cxb" && artifact.Executable != "" {
			return artifact.Executable, nil
		}
	}
	return "", fmt.Errorf("cargo reported no cinnabar-cxb executable")
}

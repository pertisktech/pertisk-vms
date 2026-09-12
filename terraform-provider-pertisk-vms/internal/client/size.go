package client

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode"
)

// ParseSize accepts values like 32G, 512MiB, or a raw byte count.
func ParseSize(input string) (uint64, error) {
	raw := strings.TrimSpace(input)
	if raw == "" {
		return 0, fmt.Errorf("size is required")
	}
	split := len(raw)
	for i, r := range raw {
		if !unicode.IsDigit(r) {
			split = i
			break
		}
	}
	if split == 0 {
		return 0, fmt.Errorf("invalid size %q", input)
	}
	n, err := strconv.ParseUint(raw[:split], 10, 64)
	if err != nil {
		return 0, fmt.Errorf("invalid size %q", input)
	}
	switch strings.ToLower(strings.TrimSpace(raw[split:])) {
	case "", "b":
		return n, nil
	case "k", "kb", "kib":
		return n * 1024, nil
	case "m", "mb", "mib":
		return n * 1024 * 1024, nil
	case "g", "gb", "gib":
		return n * 1024 * 1024 * 1024, nil
	case "t", "tb", "tib":
		return n * 1024 * 1024 * 1024 * 1024, nil
	default:
		return 0, fmt.Errorf("unknown size suffix in %q", input)
	}
}

// FormatSize renders a compact size string for Terraform state.
func FormatSize(bytes uint64) string {
	const (
		kib = 1024
		mib = 1024 * 1024
		gib = 1024 * 1024 * 1024
		tib = 1024 * 1024 * 1024 * 1024
	)
	switch {
	case bytes >= tib && bytes%tib == 0:
		return fmt.Sprintf("%dG", bytes/gib) // keep G-friendly when exact TiB
	case bytes >= gib && bytes%gib == 0:
		return fmt.Sprintf("%dG", bytes/gib)
	case bytes >= mib && bytes%mib == 0:
		return fmt.Sprintf("%dM", bytes/mib)
	case bytes >= kib && bytes%kib == 0:
		return fmt.Sprintf("%dK", bytes/kib)
	default:
		return strconv.FormatUint(bytes, 10)
	}
}

func ExpandPath(p string) string {
	p = strings.TrimSpace(p)
	if p == "" {
		return p
	}
	if strings.HasPrefix(p, "~/") {
		home, err := os.UserHomeDir()
		if err == nil {
			return filepath.Join(home, p[2:])
		}
	}
	return p
}

func InferImageFormat(path string) string {
	ext := strings.ToLower(strings.TrimPrefix(filepath.Ext(path), "."))
	switch ext {
	case "qcow2", "img":
		return "qcow2"
	default:
		return "raw"
	}
}

func FileSHA256(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

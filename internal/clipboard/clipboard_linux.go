//go:build linux

package clipboard

import (
	"errors"
	"os"
)

func getCommand() ([]string, string, error) {
	if os.Getenv("WSL_DISTRO_NAME") != "" {
		return []string{"/mnt/c/Windows/System32/clip.exe"}, "WSL", nil
	}
	if os.Getenv("WAYLAND_DISPLAY") != "" {
		return []string{"/usr/bin/wl-copy"}, "Wayland", nil
	}
	if os.Getenv("DISPLAY") != "" {
		return []string{"/usr/bin/xclip", "-sel", "clip", "-r", "-in"}, "Xorg", nil
	}
	return nil, "", errors.New("unsupported Linux environment (no WAYLAND_DISPLAY or DISPLAY)")
}

//go:build darwin

package clipboard

func getCommand() ([]string, string, error) {
	return []string{"/usr/bin/pbcopy"}, "macOS", nil
}

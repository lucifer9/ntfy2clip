//go:build windows

package clipboard

func getCommand() ([]string, string, error) {
	return []string{"clip.exe"}, "Windows", nil
}

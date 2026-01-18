package clipboard

import (
	"bytes"
	"log"
	"os/exec"
)

func Set(content string) error {
	log.Printf("Setting clipboard to: %s", content)

	cmd, envName, err := getCommand()
	if err != nil {
		return err
	}

	log.Printf("Running under %s, using copy command %s", envName, cmd[0])

	c := exec.Command(cmd[0], cmd[1:]...)
	c.Stdin = bytes.NewBufferString(content)

	return c.Run()
}

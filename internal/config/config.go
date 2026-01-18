package config

import (
	"fmt"
	"os"
	"strconv"
	"time"
)

type Config struct {
	Server  string
	Scheme  string
	Topic   string
	Token   string
	Timeout time.Duration
}

func Load() (*Config, error) {
	topic := os.Getenv("TOPIC")
	if topic == "" {
		return nil, fmt.Errorf("TOPIC environment variable is required")
	}

	server := os.Getenv("SERVER")
	if server == "" {
		server = "ntfy.sh"
	}

	scheme := os.Getenv("SCHEME")
	if scheme == "" {
		scheme = "wss"
	}

	token := os.Getenv("TOKEN")

	timeoutSec := 120
	if timeoutStr := os.Getenv("TIMEOUT"); timeoutStr != "" {
		if t, err := strconv.Atoi(timeoutStr); err == nil && t > 0 {
			timeoutSec = t
		}
	}

	return &Config{
		Server:  server,
		Scheme:  scheme,
		Topic:   topic,
		Token:   token,
		Timeout: time.Duration(timeoutSec) * time.Second,
	}, nil
}

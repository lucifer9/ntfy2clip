package main

import (
	"context"
	"log"
	"time"

	"github.com/lucifer/ntfy2clip/internal/config"
	"github.com/lucifer/ntfy2clip/internal/websocket"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("Configuration error: %v", err)
	}

	ctx := context.Background()

	for {
		if err := websocket.RunConnection(ctx, cfg); err != nil {
			log.Printf("Connection error: %v. Reconnecting...", err)
			time.Sleep(5 * time.Second)
		}
	}
}

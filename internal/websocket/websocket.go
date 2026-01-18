package websocket

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"net/http"
	"time"

	"github.com/coder/websocket"
	"github.com/lucifer/ntfy2clip/internal/clipboard"
	"github.com/lucifer/ntfy2clip/internal/config"
)

type ntfyMessage struct {
	Event   string  `json:"event"`
	Topic   string  `json:"topic"`
	Message *string `json:"message,omitempty"`
}

func dial(ctx context.Context, cfg *config.Config) (*websocket.Conn, error) {
	url := fmt.Sprintf("%s://%s/%s/ws", cfg.Scheme, cfg.Server, cfg.Topic)

	headers := http.Header{}
	if cfg.Token != "" {
		headers.Set("Authorization", "Bearer "+cfg.Token)
	}

	transport := &http.Transport{
		TLSClientConfig: &tls.Config{
			MinVersion: tls.VersionTLS12,
		},
		Proxy: http.ProxyFromEnvironment,
		DialContext: (&net.Dialer{
			Timeout: 10 * time.Second,
		}).DialContext,
	}

	opts := &websocket.DialOptions{
		HTTPHeader: headers,
		HTTPClient: &http.Client{Transport: transport},
	}

	conn, _, err := websocket.Dial(ctx, url, opts)
	return conn, err
}

func RunConnection(ctx context.Context, cfg *config.Config) error {
	dialCtx, cancel := context.WithTimeout(ctx, 15*time.Second)
	conn, err := dial(dialCtx, cfg)
	cancel()

	if err != nil {
		return fmt.Errorf("dial error: %w", err)
	}
	defer conn.Close(websocket.StatusNormalClosure, "bye")

	log.Printf("Connected to %s with topic=%s and timeout=%v", cfg.Server, cfg.Topic, cfg.Timeout)

	lastTraffic := time.Now()
	ticker := time.NewTicker(cfg.Timeout)
	defer ticker.Stop()

	readCh := make(chan readResult)

	go func() {
		for {
			msgType, data, err := conn.Read(ctx)
			select {
			case readCh <- readResult{msgType, data, err}:
			case <-ctx.Done():
				return
			}
			if err != nil {
				return
			}
		}
	}()

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()

		case result := <-readCh:
			lastTraffic = time.Now()

			if result.err != nil {
				return fmt.Errorf("read error: %w", result.err)
			}

			if result.msgType == websocket.MessageText {
				var msg ntfyMessage
				if err := json.Unmarshal(result.data, &msg); err != nil {
					log.Printf("Error parsing JSON: %v", err)
					continue
				}

				if msg.Topic == cfg.Topic && msg.Event == "message" && msg.Message != nil {
					log.Printf("WS received message: event=%s, topic=%s", msg.Event, msg.Topic)
					go func(content string) {
						if err := clipboard.Set(content); err != nil {
							log.Printf("Failed to set clipboard: %v", err)
						}
					}(*msg.Message)
				}
			}

		case <-ticker.C:
			if time.Since(lastTraffic) > cfg.Timeout {
				return fmt.Errorf("no traffic in the last %v", cfg.Timeout)
			}
		}
	}
}

type readResult struct {
	msgType websocket.MessageType
	data    []byte
	err     error
}

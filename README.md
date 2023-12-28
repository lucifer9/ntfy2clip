# ntfy2clip
This tiny app connects to [ntfy.sh](https://ntfy.sh) or a [self-hosted](https://docs.ntfy.sh/install/) ntfy server through websocket.  
When it receives a text message of a specified topic, it automatically updates  
the system clipboard with the message content.  
It's easy to modify it to work on different platforms and/or to support more message types.

Configuration is managed through Environment Variables:
- `SERVER`: your self-hosted ntfy server, or `ntfy.sh` by default
- `SCHEME`: `wss` by default, can be `ws` for servers without TLS
- `TOPIC`: to which you subscribe, multiple topics are **NOT** supported for now
- `TOKEN`: your access token, if needed

To maintain the WebSocket connection, a Ping frame is sent every 60 seconds.  
If there's no activity for over 120 seconds, the app attempts to reconnect.  
These intervals are adjustable via the `HEARTBEAT` and `TIMEOUT` environment variables.

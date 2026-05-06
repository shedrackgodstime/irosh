# Irosh CLI Command Tree

```text
irosh                              # Dashboard: show status, active wormholes, & sessions
irosh <target>                     # Shortcut: connect to a ticket or alias

irosh put <peer> <local> [remote]  # FAST: upload file/folder to peer
irosh get <peer> <remote> [local]  # FAST: download file/folder from peer

irosh wormhole                     # Pairing: create new (bare) or manage (subcommands)
  [code] [--password] [--persistent]
  [--timeout <duration>]           # Expiry override (default 24h)
  status                           # List active wormholes & their codes
  disable <code>                   # Kill a specific wormhole

irosh connect <target>             # Explicit connection with advanced flags
  [--forward L:port:R:port]        # Local/Remote port forwarding
  [--auth-password]                # Force password prompt        #i dont think this is needed
  [--insecure]                     # Skip host key verification    #and this too
  [--secret <string>]              # Use stealth shared secret    #and this should be check to see why and if it worth staying.....

irosh system                       # Service/Daemon management
  install | uninstall              # OS-level service setup (systemd/launchd/winsvc)
  start | stop | restart           # Control the background daemon
  status                           # Service health (PID, Uptime, Memory)
  logs [-f]                        # Stream daemon logs (crucial for debugging)

irosh peer                         # Address book (aliases)
  list [--full]                    # List saved peers and their NodeIDs
  add <name> <ticket>              # Manually add a peer
  remove <name>                    # Delete a saved peer
  info <name>                      # Show detailed peer metadata

irosh trust                        # Security: Authorized Keys
  list                             # Show who is allowed to connect to you
  allow <node_id> <key>            # Add a peer key to authorized_clients
  revoke <node_id>                 # Remove a peer key

irosh passwd                       # Server Authentication
  set                              # Interactive prompt to set password (secure)
  remove                           # Clear password authentication
  status                           # Check if password auth is enabled

irosh identity                     # Local machine identity
  show                             # Display your Public Key / NodeID
  rotate                           # Generate a new identity (Warning: changes ID)

irosh config                       # Global CLI settings
  get | set | list                 # Manage default-user, relay-nodes, etc.

irosh check                        # P2P diagnostics & network health
```

## Key Design Principles

- **Top-Level Velocity**: Common tasks (`put`, `get`, `wormhole`) are promoted to the top level for speed.
- **Secure by Design**: Secrets like `passwd set` use interactive TTY prompts, not CLI arguments.
- **Dual Personas**: Clearly separates "Machine A" (Server/System) tasks from "Machine B" (Client/Connect) tasks.
- **Visibility**: `system logs` and the root `dashboard` provide high-signal feedback to the user.

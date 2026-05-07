noted when testing.... after connection was made through wormhole, i was still able to use that same wormhole again... i connected twice before it now stop...
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh system install                         
[P2P] Installing background service...
2026-05-07T18:17:03.987282Z  INFO irosh::sys::unix::service: Service unit installed to /home/kristency/.config/systemd/user/irosh.service
Created symlink '/home/kristency/.config/systemd/user/default.target.wants/irosh.service' → '/home/kristency/.config/systemd/user/irosh.service'.
2026-05-07T18:17:04.641448Z  INFO irosh::sys::unix::service: Service enabled and started in the background.
[OK] Service installed and started.
[INFO] Run 'irosh system status' to verify.
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh wormhole kristency-kz --passwd 
✔ Enter wormhole session password (one-time use) · ********
[OK] Wormhole password set (will be destroyed after first use).
[P2P] Starting temporary wormhole server...
2026-05-07T18:17:16.832548Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:17:16.832775Z  INFO relay-actor: iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link./, was None
[OK] Wormhole active (Foreground)! Code: kristency-kz
[INFO] Run 'irosh kristency-kz' on the other machine.
[INFO] Waiting for peer... (Ctrl+C to cancel)
2026-05-07T18:17:16.833079Z  INFO irosh::server: Server actively listening for connections.
2026-05-07T18:17:16.833108Z  INFO irosh::server: Wormhole enabled via IPC: kristency-kz (persistent: false)
2026-05-07T18:17:17.875143Z  INFO mainline::dht: Mainline DHT listening address=0.0.0.0:6881
2026-05-07T18:17:17.875487Z  INFO irosh::transport::wormhole: 📡 Publishing wormhole to Pkarr rendezvous: kristency-kz
2026-05-07T18:17:38.087831Z  INFO irosh::server: Established bi-directional SSH stream over Irosh
2026-05-07T18:17:38.087919Z  INFO irosh::server: Pairing connection established via wormhole code.
2026-05-07T18:17:43.919084Z  INFO irosh::auth: Password accepted: Adding new client to vault. fingerprint=SHA256:yCszP8JvUQqsNQVjtiDV0jVaJdo1VxiBVsTKmQHw+g4
2026-05-07T18:17:50.479449Z  INFO irosh::server::handler: pty_request for channel ChannelId(2): term=xterm-256color, cols=190, rows=40
2026-05-07T18:17:50.479544Z  INFO irosh::server::handler: shell_request for channel ChannelId(2)
2026-05-07T18:17:50.483829Z  INFO irosh::server::handler::pty: Spawned PTY child for channel ChannelId(2): command=None, pid=Some(894746)
2026-05-07T18:17:50.483892Z  INFO irosh::server::handler::pty: Registering PRIMARY shell PID Some(894746) for session state
2026-05-07T18:17:50.483912Z  INFO irosh::server::transfer::state: Connection state update: Shell PID registered as Some(894746)
2026-05-07T18:17:50.484776Z  INFO irosh::server::handler::pty: Waiting for child process Some(894746) for channel ChannelId(2)
2026-05-07T18:18:16.956777Z  INFO irosh::server::handler::pty: Child process Some(894746) for channel ChannelId(2) exited with code 0
2026-05-07T18:18:16.956920Z  INFO irosh::server::transfer::state: Connection state update: Clearing shell PID Some(894746)
2026-05-07T18:18:31.071746Z  INFO irosh::server: Established bi-directional SSH stream over Irosh
2026-05-07T18:18:31.071792Z  INFO irosh::server: Pairing connection established via wormhole code.
2026-05-07T18:18:31.072599Z  INFO irosh::server: Wormhole successfully consumed. Auto-burning.
2026-05-07T18:18:31.088305Z  INFO mainline::dht: Mainline DHT listening address=0.0.0.0:39382
2026-05-07T18:18:31.308221Z  INFO irosh::auth: Client matched pre-authorized key. Access granted. fingerprint=SHA256:yCszP8JvUQqsNQVjtiDV0jVaJdo1VxiBVsTKmQHw+g4
2026-05-07T18:18:31.310515Z  INFO irosh::server::handler: pty_request for channel ChannelId(2): term=xterm-256color, cols=190, rows=40
2026-05-07T18:18:31.310620Z  INFO irosh::server::handler: shell_request for channel ChannelId(2)
2026-05-07T18:18:31.313882Z  INFO irosh::server::handler::pty: Spawned PTY child for channel ChannelId(2): command=None, pid=Some(895229)
2026-05-07T18:18:31.313908Z  INFO irosh::server::handler::pty: Registering PRIMARY shell PID Some(895229) for session state
2026-05-07T18:18:31.313919Z  INFO irosh::server::transfer::state: Connection state update: Shell PID registered as Some(895229)
2026-05-07T18:18:31.314221Z  INFO irosh::server::handler::pty: Waiting for child process Some(895229) for channel ChannelId(2)
2026-05-07T18:19:47.039163Z  WARN iroh_quinn_proto::connection: sent PATH_ABANDON after path was already discarded
2026-05-07T18:19:50.184990Z  WARN iroh_quinn_proto::connection: sent PATH_ABANDON after path was already discarded
2026-05-07T18:20:04.889274Z  INFO irosh::server::handler::pty: Child process Some(895229) for channel ChannelId(2) exited with code 0
2026-05-07T18:20:04.889377Z  INFO irosh::server::transfer::state: Connection state update: Clearing shell PID Some(895229)
2026-05-07T18:20:25.050595Z  INFO irosh::server: Established bi-directional SSH stream over Irosh
2026-05-07T18:20:25.050654Z  WARN irosh::server: Pairing connection attempted but no wormhole active.
2026-05-07T18:20:48.209959Z  INFO irosh::server: Established bi-directional SSH stream over Irosh
2026-05-07T18:20:48.210052Z  WARN irosh::server: Pairing connection attempted but no wormhole active.


it was suppose to terminate self..... we need to think about it again... we have to think of a better desing... since it seperate process, immidiately connection is succesful it should terminate.. thats how we suppose to build it... the main background process should now be the one handly the shh..... but the problem now is how we gonna impliment it with the host thats running in forground.......




########################################################################
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh --state ./test-client connect
✔ Select a peer to connect · [peer-45c10533] endpointabc4...2lonmxc6
[INFO] Connecting to saved peer: peer-45c10533
⢀ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:11.296598Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:50:11.296807Z  INFO relay-actor: iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link./, was None
⠄ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:12.638233Z  WARN RemoteStateActor{me=682361e4b4 remote=45c1053342}: iroh::socket::remote_map::remote_state: Address Lookup failed: Service 'dns' error: no calls succeeded: [Failed to resolve TXT recordFailed to resolve TXT recordFailed to resolve TXT record]
⠠ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:17.799277Z  WARN connect{me=682361e4b4 alpn="irosh/1" remote=45c1053342}: iroh_quinn_proto::connection: failed closing path err=ClosedPath
⠄ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:27.866262Z  WARN connect{me=682361e4b4 alpn="irosh/1" remote=45c1053342}: iroh_quinn_proto::connection: remote server configuration might cause nat traversal issues max_local_addresses=12 remote_cid_limit=5
⠁ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:50:31.660467Z  INFO irosh::client::handler: Server matched trusted key. Connection verified.
✔ Public key rejected. Server requires a password · ********
Error: authentication failed
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh --state ./test-client connect
✔ Select a peer to connect · [peer-45c10533] endpointabc4...2lonmxc6
[INFO] Connecting to saved peer: peer-45c10533
⢀ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:53.372315Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:50:53.372351Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:50:53.372573Z  INFO relay-actor: iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link./, was None
⠈ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:50:54.492656Z  WARN connect{me=682361e4b4 alpn="irosh/1" remote=45c1053342}: iroh_quinn_proto::connection: remote server configuration might cause nat traversal issues max_local_addresses=12 remote_cid_limit=5
⠂ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:50:54.630086Z  WARN RemoteStateActor{me=682361e4b4 remote=45c1053342}: iroh::socket::remote_map::remote_state: Address Lookup failed: Service 'dns' error: no calls succeeded: [Failed to resolve TXT recordFailed to resolve TXT recordFailed to resolve TXT record]
⠠ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:50:55.086444Z  INFO irosh::client::handler: Server matched trusted key. Connection verified.
✔ Public key rejected. Server requires a password · ********
Error: authentication failed
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh --state ./test-client connect
✔ Select a peer to connect · [peer-45c10533] endpointabc4...2lonmxc6
[INFO] Connecting to saved peer: peer-45c10533
⠠ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:51:34.617840Z  INFO relay-actor: iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link./, was None
⠄ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:51:35.930255Z  WARN RemoteStateActor{me=682361e4b4 remote=45c1053342}: iroh::socket::remote_map::remote_state: Address Lookup failed: Service 'dns' error: no calls succeeded: [Failed to resolve TXT recordFailed to resolve TXT recordFailed to resolve TXT record]
⠠ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:51:36.283608Z  WARN connect{me=682361e4b4 alpn="irosh/1" remote=45c1053342}: iroh_quinn_proto::connection: remote server configuration might cause nat traversal issues max_local_addresses=12 remote_cid_limit=5
⠐ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:51:37.174002Z  INFO irosh::client::handler: Server matched trusted key. Connection verified.
✔ Public key rejected. Server requires a password · ********
Error: authentication failed
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh --state ./test-client connect
✔ Select a peer to connect · [peer-45c10533] endpointabc4...2lonmxc6
[INFO] Connecting to saved peer: peer-45c10533
⢀ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:52:26.121009Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:52:26.121045Z  WARN actor: iroh::net_report::report: IPv4 address detected by QAD varies by destination
2026-05-07T18:52:26.121384Z  INFO relay-actor: iroh::socket::transports::relay::actor: home is now relay https://euc1-1.relay.n0.iroh-canary.iroh.link./, was None
⠂ Dialing P2P node...                                                                                                                                                                         2026-05-07T18:52:27.398137Z  WARN connect{me=682361e4b4 alpn="irosh/1" remote=45c1053342}: iroh_quinn_proto::connection: remote server configuration might cause nat traversal issues max_local_addresses=12 remote_cid_limit=5
⠂ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:52:27.413806Z  WARN RemoteStateActor{me=682361e4b4 remote=45c1053342}: iroh::socket::remote_map::remote_state: Address Lookup failed: Service 'dns' error: no calls succeeded: [Failed to resolve TXT recordFailed to resolve TXT recordFailed to resolve TXT record]
⠁ Performing SSH handshake...                                                                                                                                                                 2026-05-07T18:52:28.129376Z  INFO irosh::client::handler: Server matched trusted key. Connection verified.
[OK] Secure session established with kristency-linux
┏━(Message from Kali developers)
┃
┃ This is a minimal installation of Kali Linux, you likely
┃ want to install supplementary tools. Learn how:
┃ ⇒ https://www.kali.org/docs/troubleshooting/common-minimum-setup/
┃
┗━(Run: “touch ~/.hushlogin” to hide this message)
┌──(kristency㉿ALPHA-01)-[~]
└─$ 


i had to restart for it to now work which is very bad....


                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh system status                          

  ⚙️  System Service Status
  ────────────────────────────────────────────────────
  Status:    ● ACTIVE
  Manager:   systemd
  ────────────────────────────────────────────────────

                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh passwd set   
✔ Enter new password · ********
[OK] Node Password has been updated and hashed securely.
[INFO] New unknown devices will now be required to enter this password to pair.
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ irosh system restart                         
2026-05-07T18:52:20.332301Z  INFO irosh::sys::unix::service: Service stopped.
2026-05-07T18:52:20.346483Z  INFO irosh::sys::unix::service: Service started.
[OK] Service restarted.
                                                                                                                                                                                              
┌──(kristency㉿ALPHA-01)-[~/Projects/irosh/cli/test]
└─$ 




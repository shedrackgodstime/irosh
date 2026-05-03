use portable_pty::{CommandBuilder, native_pty_system, PtySize};
use std::io::{Read, Write};

fn main() {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }).unwrap();

    let mut builder = CommandBuilder::new("cmd.exe");
    let mut child = pair.slave.spawn_command(builder).unwrap();

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();

    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        println!("Reader thread started");
        match reader.read(&mut buf) {
            Ok(n) => {
                println!("Read {} bytes", n);
                tx.send(buf[..n].to_vec()).unwrap();
            }
            Err(e) => println!("Error: {:?}", e),
        }
    });

    let data = rx.recv().unwrap();
    println!("Received from pty: {:?}", String::from_utf8_lossy(&data));
    
    // Test writing
    writer.write_all(b"dir\r\n").unwrap();
    println!("Done!");
}

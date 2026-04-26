use rand::RngCore;

fn main() {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    println!("Random bytes: {:?}", bytes);
}

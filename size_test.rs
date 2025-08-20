use std::mem;

fn main() {
    println!("RaftError size: {} bytes", mem::size_of::<iroh_raft::error::RaftError>());
}

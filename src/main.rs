use std::{
    collections::hash_map::DefaultHasher,
    fs::File,
    hash::Hasher,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::{self, Builder, sleep, JoinHandle},
    time::Duration,
};
#[cfg(target_os="linux")]
use std::os::unix::ffi::OsStrExt;

fn main() {
    host(TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 80))).unwrap());
}
fn host(stream: TcpListener) {
    const MAX_THREADS: usize = 16;
    let thread_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    println!("Now hosting on {}", stream.local_addr().unwrap());
    for client in stream.incoming().flatten() {
        if let Ok(threads) = thread_count.lock() {
            if threads.lt(&MAX_THREADS) {
                if thread::Builder::new()
                    .name(
                        client
                            .peer_addr()
                            .map(|ip| ip.to_string())
                            .unwrap_or("Err".to_owned()),
                    )
                    .spawn(move || serve_client(client))
                    .is_err()
                {
                    println!("Encountered an error serving client.");
                }
            }
        }
    }
}
fn serve_client(mut client: TcpStream) {
    let mut first_line = String::default();
    loop {
        let mut char = [0u8];
        if client.read_exact(&mut char).is_ok() {
            if char == *b"\n" {
                break;
            }
            first_line.push(char::from(char[0]));
        } else {
            return;
        }
    }
    if let Ok(parsed_data) = prse::try_parse!(first_line, "{} {} HTTP{}") {
        let (method, path, _): (String, String, String) = parsed_data;
        match method.as_str() {
            "PUT" => {
                let mut was_newline = 0;
                loop {
                    let mut char_val = [0u8];
                    if client.read_exact(&mut char_val).is_ok() {
                        if char_val == *b"\r" && was_newline % 2 == 0 {
                            was_newline += 1;
                        } else if char_val == *b"\n" && was_newline % 2 == 1 {
                            was_newline += 1;
                            if was_newline == 4 {
                                break;
                            }
                        } else {
                            was_newline = 0;
                        }
                    } else {
                        return;
                    }
                }
                let mut remaining_data = Vec::new();
                let _ = client.set_nonblocking(true);
                let _ = client.set_read_timeout(Some(Duration::from_secs(3)));
                let _ = client.read_to_end(&mut remaining_data);
                let success = put(&path, &remaining_data);
                if let Ok(my_ip) = client.local_addr() {
                    if let Some(response) = success {
                        garbage_collect(PathBuf::from(&response)).unwrap();
                        let response = format!("HTTP/1.1 200 Your file should be accessible at \"http://{0}/{1}\" for the next hour, to download the file, run:\r\n$ curl http://{0}/{1} | cat > FILENAME.txt\r\n", my_ip, response.to_string());
                        client.write_all(response.as_bytes()).unwrap();
                    }
                }
            }
            "GET" => {
                let success = get(&path);
                if let Some(response) = success {
                    client.write_all(format!("HTTP/1.1 200 Ok\r\nContent-Length: {}\r\n\r\n", response.len()).as_bytes()).unwrap();
                    client.write_all(&response).unwrap();
                } else {
                    client.write_all(format!("HTTP/1.1 409 Failed to get file at {path}\r\n").as_bytes()).unwrap();
                }
            }
            "DELETE" => {
                let response = if delete(&path) {
                    format!("HTTP/1.1 200 The file hosted at {} has been removed\r\n\r\n", {path})
                } else {
                    format!("HTTP/1.1 409 The file hosted at {} could not be removed\r\n\r\n", {path})
                };
                client.write_all(response.as_bytes()).unwrap();
            }
            _ => return,
        }
    }
}
#[cfg(target_os="windows")]
fn put<T: AsRef<std::path::Path>>(path: T, data: &[u8]) -> Option<String> {
    let name = {
        let mut hasher = DefaultHasher::new();
        let _ = hasher.write(path.as_ref().as_os_str().as_encoded_bytes());
        let _ = hasher.write(&data);
        let rand: usize = rand::Rng::gen(&mut rand::thread_rng());
        let _ = hasher.write(&rand.to_be_bytes());
        let val = hasher.finish();
        format!("{val:0x}")
    };
        let mut file_name = Path::new("files/").to_path_buf();
        file_name.push(name.clone());
        if !file_name.exists() {
            File::create(file_name).unwrap().write_all(&data).ok()?;
            return Some(name);
        }
    None
}
#[cfg(target_os="linux")]
fn put<T: AsRef<std::path::Path>>(path: T, data: &[u8]) -> Option<String> {
    let name = {
        let mut hasher = DefaultHasher::new();
        let _ = hasher.write(path.as_ref().as_os_str().as_bytes());
        let _ = hasher.write(&data);
        let rand: usize = rand::Rng::gen(&mut rand::thread_rng());
        let _ = hasher.write(&rand.to_be_bytes());
        let val = hasher.finish();
        format!("{val:0x}")
    };
        let mut file_name = Path::new("files/").to_path_buf();
        file_name.push(name.clone());
        if !file_name.exists() {
            File::create(file_name).unwrap().write_all(&data).ok()?;
            return Some(name);
        }
    None
}
fn get<T: AsRef<std::path::Path>>(path: T) -> Option<Vec<u8>> {
    let mut file_name = Path::new("files/").to_path_buf();
        file_name.push(path.as_ref().file_name().unwrap());
    if file_name.is_file() {
        let mut file_data = Vec::new();
        File::open(file_name).unwrap().read_to_end(&mut file_data).ok()?;
        Some(file_data)
    } else {
        None
    }
}
fn delete<T: AsRef<std::path::Path>>(path: T) -> bool {
    let mut file_name = Path::new("files/").to_path_buf();
    file_name.push(path.as_ref().file_name().unwrap());
    std::fs::remove_file(file_name).is_ok()
}
fn garbage_collect(path: PathBuf) -> Option<JoinHandle<()>> where {
    Builder::new().name(path.to_str()?.to_string()).spawn(move || {
        sleep(Duration::from_secs(60*60));
        if !delete(&path) {
            println!("Deleted {path:?}");
        } else {
            println!("Failed to delete {path:?}");
        }
    }).ok()
}
use std::{
    collections::hash_map::DefaultHasher,
    fs::{read_dir, File},
    hash::Hasher,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream},
    path::Path,
    sync::{Arc, Mutex},
    thread::{self, sleep, Builder},
    time::Duration,
};
#[cfg(target_os="linux")]
use std::os::unix::ffi::OsStrExt;

fn main() {
    host(TcpListener::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 80))).expect("Failed to bind to port, try running as sudo."));
}
fn host(stream: TcpListener) {
    garbage_collect();
    const MAX_THREADS: usize = 16;
    let thread_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    println!("Now hosting on http://{}/", stream.local_addr().map(|ip| ip.to_string())
    .unwrap_or("Err".to_owned()));
    for client in stream.incoming().flatten() {
        if let Ok(addr) = client.peer_addr() {
            println!("Request from {addr}");
        } else {
            println!("Request from unknown peer");
        }
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
    let _ = client.set_read_timeout(Some(Duration::from_millis(300)));
    let _ = client.read_to_end(&mut Vec::new());
    if let Ok(parsed_data) = prse::try_parse!(first_line, "{} {} HTTP{}") {
        let (method, path, protocol): (String, String, String) = parsed_data;
        println!("Client request: {method} {path} HTTP{protocol}");
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
                // let _ = client.set_nonblocking(true);
                let _ = client.set_read_timeout(Some(Duration::from_millis(300)));
                let _ = client.read_to_end(&mut remaining_data);
                let success = put(&path, &remaining_data);
                if let Ok(my_ip) = client.local_addr() {
                    if let Some(response) = success {
                        let response = format!("HTTP/1.1 200 Your file should be accessible at: \"http://{0}/{1}\" For the next hour.\r\n To download the file, run:\r\n$ curl http://{0}/{1} --output filename.txt\r\n", my_ip, response.to_string());
                        let _ = client.write_all(response.as_bytes());
                    }
                }
            }
            "GET" => {
                if path == "/" {
                    if let Ok(page_data) = std::fs::read("src/index.html") {
                        send_data(&page_data, &mut client, ResponseCode::Ok);
                    }
                } else if path == "/styles.css" {
                    if let Ok(page_data) = std::fs::read("src/styles.css") {
                        send_data(&page_data, &mut client, ResponseCode::Ok);
                    }
                } else if path == "/favicon.ico" {
                    if let Ok(page_data) = std::fs::read("src/cuddlyferris.ico") {
                        send_data(&page_data, &mut client, ResponseCode::Ok);
                    }
                } else {
                    let success = get(&path);
                    if let Some(response) = success {
                        send_data(&response, &mut client, ResponseCode::Ok);
                        // println!("Provided {path} successfully: \"{response:#?}\"");
                    } else {
                        let response = format!("HTTP/1.1 404 File \"{path}\" not found\r\n");
                        send_data(
                            &response.as_bytes(),
                            &mut client,
                            ResponseCode::FileNotFound,
                        );
                    }
                }
            }
            "DELETE" => {
                let response = if delete(&path) {
                    format!(
                        "HTTP/1.1 200 The file hosted at {} has been removed\r\n\r\n",
                        { path }
                    )
                } else {
                    format!(
                        "HTTP/1.1 404 The file hosted at {} could not be removed\r\n\r\n",
                        { path }
                    )
                };
                    let _ = client.write_all(response.as_bytes());
            }
            _ => return,
        }
    } else {
        println!("Invalid request: {first_line}");
    }
    let _ = client.shutdown(std::net::Shutdown::Both);
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
        File::create(file_name).ok()?.write_all(&data).ok()?;
        return Some(name);
    }
    None
}
fn get<T: AsRef<std::path::Path>>(path: T) -> Option<Vec<u8>> {
    let mut file_name = Path::new("files/").to_path_buf();
    file_name.push(path.as_ref().file_name()?);
    if file_name.is_file() {
        let mut file_data = Vec::new();
        File::open(file_name)
            .ok()?
            .read_to_end(&mut file_data)
            .ok()?;
        Some(file_data)
    } else {
        None
    }
}
fn delete<T: AsRef<std::path::Path>>(path: T) -> bool {
    let mut file_name = Path::new("files/").to_path_buf();
    if let Some(name) = path.as_ref().file_name() {
        file_name.push(name);
    }
    std::fs::remove_file(file_name).is_ok()
}
fn garbage_collect() {
    Builder::new()
        .name("Garbage collector".to_string())
        .spawn(move || {
            loop {
                for file in read_dir("files").expect("Garbage collector failed to start, aborting.").into_iter() {
                    if let Ok(file) = file {
                        if let Ok(Ok(Ok(secs_elapsed))) = file.metadata().map(|meta| {
                            meta.created()
                                .map(|date| date.elapsed().map(|elapsed| elapsed.as_secs()))
                        }) {
                            if secs_elapsed > 3600 {
                                match  std::fs::remove_file(file.path()) {
                                    Ok(_) => println!("Deleted {}", file.path().display()),
                                    Err(val) => println!("Failed to delete {}: {val}", file.path().display()),
                                }
                            }
                        }
                    }
                }
                sleep(Duration::from_secs(5));
            }
        }).expect("Garbage collector failed to start, aborting.");
}
fn send_data(contents: &[u8], stream: &mut TcpStream, response_code: ResponseCode) {
    let status_line = match response_code {
        ResponseCode::Ok => "HTTP/1.1 200 OK",
        ResponseCode::FileNotFound => "HTTP/1.1 404 File not found",
    };
    let length = contents.len();
    println!("len: {}", contents.len());
    let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n");
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&contents);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}
enum ResponseCode {
    Ok,
    FileNotFound,
}

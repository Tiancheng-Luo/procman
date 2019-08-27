use procman::*;
use std::io::Result;
use std::process::Command;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread;

#[test]
fn test_run() {
    let man = ProcessManager::new();
    let inner = man.clone();
    let flag: Arc<RwLock<Option<Vec<u8>>>> = Default::default();
    let inner_flag = flag.clone();

    thread::spawn(move || {
        inner.run_process_with_intercept(
            "foo".to_string(),
            Command::new("echo").arg("hello"),
            move |ev: ProcessEvent, k: &dyn Fn(ProcessEvent) -> Result<()>| {
                println!("event: {}", ev);
                if let ProcessEvent::Output(_handle, bytes, len) = &ev {
                    if *len > 0 {
                        *inner_flag.write().unwrap() = Some({
                            let mut b = bytes.clone();
                            b.truncate(*len);
                            b
                        })
                    }
                };
                k(ev)
            },
        )
    });

    println!("running the directory");
    man.run_director().expect("run_director failed");

    let mv = flag.read().unwrap();
    let v = mv.as_ref().unwrap();
    assert_eq!(&v[..v.len()], "hello\n".as_bytes());
}

use constellation::background::Latest;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
#[test]
fn pending_work_is_replaced_and_stale_results_never_surface() {
    let (started_tx, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (done, ready) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let worker = Latest::new(
        move |key: &u32| {
            seen.lock().unwrap().push(*key);
            if *key == 1 {
                started_tx.send(()).unwrap();
                released.recv().unwrap();
            }
            key * 10
        },
        move || {
            let _ = done.send(());
        },
    )
    .unwrap();
    assert_eq!(worker.get(&1), None);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(worker.get(&2), None);
    assert_eq!(worker.get(&3), None);
    release.send(()).unwrap();
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(worker.get(&3), Some(30));
    assert_eq!(*calls.lock().unwrap(), vec![1, 3]);
    worker.invalidate();
    assert_eq!(worker.get(&3), None);
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(worker.get(&3), Some(30));
}

#[test]
fn invalidating_an_inflight_key_does_not_publish_its_old_result() {
    let (started_tx, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (done, ready) = mpsc::channel();
    let mut calls = 0;
    let worker = Latest::new(
        move |_: &u32| {
            calls += 1;
            if calls == 1 {
                started_tx.send(()).unwrap();
                released.recv().unwrap();
            }
            calls
        },
        move || {
            let _ = done.send(());
        },
    )
    .unwrap();
    worker.get(&1);
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    worker.invalidate();
    assert_eq!(worker.get(&1), None);
    release.send(()).unwrap();
    ready.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(worker.get(&1), Some(2));
    assert!(ready.try_recv().is_err());
}

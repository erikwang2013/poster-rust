//! 存储后端：MemoryStorage / FileStorage（+ feature = "redis" 时的 RedisStorage）。
//!
//! Redis 用例在**连不上时跳过而不是失败**（本机没装 Redis 也要能跑 `cargo test`）。

use std::sync::Arc;
use std::time::Duration;

use poster::PosterConfig;
use poster::captcha::CaptchaManager;
use poster::storage::{FileStorage, MemoryStorage, Storage, default_dir};

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("poster-captcha-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    dir
}

#[test]
fn memory_round_trip_and_ttl() {
    let storage = MemoryStorage::new();
    let key = "captcha:1";

    storage.set(key, b"v1", Duration::from_secs(60)).unwrap();
    let entry = storage.get(key).unwrap().expect("刚写进去的应读得到");
    assert_eq!(entry.value, b"v1");
    let ttl = entry.ttl.expect("内存存储应报告剩余 TTL");
    assert!(ttl <= Duration::from_secs(60) && ttl > Duration::from_secs(55));

    // set 会重置尝试计数
    assert_eq!(storage.increment(key).unwrap(), Some(1));
    assert_eq!(storage.increment(key).unwrap(), Some(2));
    storage.set(key, b"v2", Duration::from_secs(60)).unwrap();
    assert_eq!(storage.increment(key).unwrap(), Some(1));

    storage.delete(key).unwrap();
    assert!(storage.get(key).unwrap().is_none());
    assert!(storage.is_empty());
    // 键不存在：increment 返回 None（不是 0，也不是 Some(1)）
    assert_eq!(storage.increment(key).unwrap(), None);
}

#[test]
fn memory_expires() {
    let storage = MemoryStorage::new();
    storage.set("k", b"v", Duration::from_millis(400)).unwrap();
    assert!(storage.get("k").unwrap().is_some());

    std::thread::sleep(Duration::from_millis(700));
    assert!(storage.get("k").unwrap().is_none(), "过期后读不到");
    assert_eq!(storage.increment("k").unwrap(), None, "过期后自增拿不到序号");
}

#[test]
fn file_round_trip_and_ttl() {
    let dir = tmp_dir("file-round-trip");
    let storage = FileStorage::new(&dir).unwrap();

    storage.set("k", b"value", Duration::from_secs(60)).unwrap();
    assert_eq!(storage.get("k").unwrap().unwrap().value, b"value");
    assert_eq!(storage.increment("k").unwrap(), Some(1));
    assert_eq!(storage.increment("k").unwrap(), Some(2));
    // 重新 set 重置计数（同 PHP）
    storage.set("k", b"value2", Duration::from_secs(60)).unwrap();
    assert_eq!(storage.increment("k").unwrap(), Some(1));

    // 短 TTL 落盘后过期即读不到，文件也被清掉
    storage.set("short", b"v", Duration::from_millis(400)).unwrap();
    assert!(storage.get("short").unwrap().is_some());
    std::thread::sleep(Duration::from_millis(700));
    assert!(storage.get("short").unwrap().is_none());
    assert!(!dir.join("short.json").exists(), "过期条目应删除文件");

    storage.delete("k").unwrap();
    assert!(storage.get("k").unwrap().is_none());
}

#[test]
fn file_keys_are_escaped_and_cannot_escape_the_directory() {
    let dir = tmp_dir("file-traversal");
    let storage = FileStorage::new(&dir).unwrap();

    // 除 [A-Za-z0-9_-] 外一律转义（含 `.`），`..` 这种文件名根本拼不出来
    for (key, expected_name) in [
        ("../escape", "%2E%2E%2Fescape.json"),
        ("/etc/passwd", "%2Fetc%2Fpasswd.json"),
        ("a/../../b", "a%2F%2E%2E%2F%2E%2E%2Fb.json"),
        ("..\\windows", "%2E%2E%5Cwindows.json"),
        ("with space", "with%20space.json"),
    ] {
        storage.set(key, b"v", Duration::from_secs(60)).unwrap();
        assert_eq!(storage.get(key).unwrap().unwrap().value, b"v", "key = {key}");
        assert_eq!(storage.increment(key).unwrap(), Some(1), "key = {key}");

        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![expected_name.to_string()], "key = {key}");
        assert!(
            !dir.parent().unwrap().join("escape.json").exists(),
            "不得在上级目录建文件"
        );

        storage.delete(key).unwrap();
    }
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none(), "清空后目录为空");
}

#[test]
fn file_increment_is_thread_safe_within_the_process() {
    let dir = tmp_dir("file-concurrency");
    let storage = Arc::new(FileStorage::new(&dir).unwrap());
    storage.set("counter", b"v", Duration::from_secs(60)).unwrap();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let storage = Arc::clone(&storage);
        handles.push(std::thread::spawn(move || {
            for _ in 0..50 {
                let attempts = storage.increment("counter").unwrap().expect("计数应可自增");
                assert!(attempts <= 400);
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    // 进程内读改写串行化：8 × 50 次一次不丢
    assert_eq!(storage.increment("counter").unwrap(), Some(401));
}

#[test]
fn default_dir_follows_config() {
    let config = PosterConfig::default();
    assert!(default_dir(&config).ends_with("poster-captcha"), "未配置时用系统临时目录");

    let mut config = config;
    config.captcha.file_path = Some("/tmp/poster-captcha-custom".into());
    assert_eq!(
        default_dir(&config),
        std::path::PathBuf::from("/tmp/poster-captcha-custom")
    );
}

#[test]
fn probe_round_trips_and_leaves_nothing_behind() {
    let dir = tmp_dir("probe");
    let storage = Arc::new(FileStorage::new(&dir).unwrap());
    let manager = CaptchaManager::with_storage(storage);

    manager.probe().expect("可用存储应探针通过");
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "探针写入的键应被清掉"
    );
}

#[test]
fn probe_fails_when_the_storage_is_unusable() {
    let dir = tmp_dir("probe-broken");
    let storage = Arc::new(FileStorage::new(&dir).unwrap());
    let manager = CaptchaManager::with_storage(storage);

    // 目录被删掉后写盘必失败：接线期就该报错，而不是等第一个请求
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(manager.probe().is_err());
}

#[cfg(feature = "redis")]
#[test]
fn redis_round_trip_or_skip() {
    use poster::storage::RedisStorage;

    let prefix = format!("poster:test:{}:", std::process::id());
    let Ok(storage) = RedisStorage::new("redis://127.0.0.1:6379/", &prefix) else {
        eprintln!("跳过：本机没有可用的 Redis（redis://127.0.0.1:6379/）");
        return;
    };

    storage.set("k", b"v", Duration::from_secs(30)).unwrap();
    let entry = storage.get("k").unwrap().expect("刚写进去的应读得到");
    assert_eq!(entry.value, b"v");
    assert!(entry.ttl.is_none(), "Redis 的 TTL 由服务端维护，不额外往返查询");

    assert_eq!(storage.increment("k").unwrap(), Some(1));
    assert_eq!(storage.increment("k").unwrap(), Some(2));
    storage.set("k", b"v2", Duration::from_secs(30)).unwrap();
    assert_eq!(storage.increment("k").unwrap(), Some(1), "set 应重置计数");

    // 键不存在：INCR 出来的计数键要清掉，返回 None（失败关闭）
    assert_eq!(storage.increment("missing").unwrap(), None);

    storage.delete("k").unwrap();
    assert!(storage.get("k").unwrap().is_none());
}

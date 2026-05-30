//! Unit tests for the `ProcessHost` trait surface. The native impl tests are
//! gated on the `native-proc` feature so they are skipped in WASM and in
//! default-feature CI runs.

use wat_core::process::{ChildStdin, NoopProcessHost, ProcessError, ProcessHost, ProcessSpec};

#[test]
fn noop_lookup_always_returns_none() {
    let host = NoopProcessHost;
    assert!(host.lookup("ls").is_none());
    assert!(host.lookup("definitely-not-real").is_none());
}

#[test]
fn noop_spawn_returns_unsupported() {
    let host = NoopProcessHost;
    let spec = ProcessSpec {
        argv: vec!["echo".to_string()],
        env: vec![],
        cwd: std::env::temp_dir(),
    };
    let res = host.spawn(spec, ChildStdin::Null);
    match res {
        Err(ProcessError::Unsupported) => {}
        _ => panic!("expected ProcessError::Unsupported"),
    }
}

#[cfg(feature = "native-proc")]
mod native {
    use wat_core::process::{
        ChildProcess as _, ChildStdin, NativeProcessHost, ProcessHost, ProcessSpec,
    };

    #[test]
    fn lookup_finds_ls() {
        let host = NativeProcessHost;
        let p = host.lookup("ls").expect("`ls` should be on PATH");
        assert!(p.exists(), "`ls` resolved to non-existent path: {:?}", p);
    }

    #[test]
    fn lookup_misses_for_garbage_name() {
        let host = NativeProcessHost;
        assert!(host
            .lookup("definitely-not-a-real-command-xyz-9999")
            .is_none());
    }

    #[test]
    fn spawn_echo_produces_expected_output() {
        let host = NativeProcessHost;
        let echo = host.lookup("echo").expect("`echo` should be on PATH");
        let spec = ProcessSpec {
            argv: vec![
                echo.to_string_lossy().into(),
                "hello".into(),
                "world".into(),
            ],
            env: vec![],
            cwd: std::env::temp_dir(),
        };
        let mut child = host.spawn(spec, ChildStdin::Null).expect("spawn ok");

        // Drain stdout
        let mut out = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            let n = child.read_stdout(&mut buf).expect("read stdout");
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        let code = child.wait().expect("wait ok");

        assert_eq!(code, 0);
        assert_eq!(String::from_utf8_lossy(&out).trim(), "hello world");
    }

    #[test]
    fn spawn_false_returns_nonzero_exit() {
        let host = NativeProcessHost;
        let Some(false_bin) = host.lookup("false") else {
            // `false` is part of coreutils — skip if not available on this host.
            eprintln!("`false` not on PATH; skipping");
            return;
        };
        let spec = ProcessSpec {
            argv: vec![false_bin.to_string_lossy().into()],
            env: vec![],
            cwd: std::env::temp_dir(),
        };
        let mut child = host.spawn(spec, ChildStdin::Null).expect("spawn ok");
        let code = child.wait().expect("wait ok");
        assert_ne!(code, 0);
    }
}

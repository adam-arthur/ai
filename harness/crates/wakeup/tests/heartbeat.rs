use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use wakeup::{Heartbeat, HeartbeatError, HeartbeatOutcome};

#[tokio::test]
async fn heartbeat_only_runs_effect_when_condition_is_true() {
    let calls = Arc::new(AtomicUsize::new(0));
    let effect_calls = Arc::clone(&calls);
    let heartbeat = Heartbeat::new(
        Duration::from_secs(1),
        || async { Ok(true) },
        move || {
            let effect_calls = Arc::clone(&effect_calls);
            async move {
                effect_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    );

    assert_eq!(heartbeat.poll_once().await.unwrap(), HeartbeatOutcome::EffectExecuted);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let heartbeat = Heartbeat::new(
        Duration::from_secs(1),
        || async { Ok(false) },
        || async { Err(HeartbeatError::new("must not run")) },
    );
    assert_eq!(heartbeat.poll_once().await.unwrap(), HeartbeatOutcome::ConditionFalse);
}

#[tokio::test]
async fn heartbeat_polls_repeatedly_until_shutdown() {
    let calls = Arc::new(AtomicUsize::new(0));
    let effect_calls = Arc::clone(&calls);
    let heartbeat = Heartbeat::new(
        Duration::from_millis(5),
        || async { Ok(true) },
        move || {
            let effect_calls = Arc::clone(&effect_calls);
            async move {
                effect_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    );

    heartbeat
        .run_until(async { tokio::time::sleep(Duration::from_millis(24)).await })
        .await
        .unwrap();

    assert!(calls.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn zero_heartbeat_interval_returns_an_error() {
    let heartbeat = Heartbeat::new(Duration::ZERO, || async { Ok(false) }, || async { Ok(()) });

    let error = heartbeat.run_until(async {}).await.unwrap_err();
    assert_eq!(
        error,
        HeartbeatError::new("heartbeat interval must be greater than zero")
    );
}

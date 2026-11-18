use std::sync::{Arc, Barrier};
use wasmdroid_engine::{
    EngineError,
    operation::{OperationControl, OperationState},
};

#[test]
fn a_rejected_second_claim_cannot_finalize_the_current_owner() {
    let control = Arc::new(OperationControl::default());
    let guard = control.claim().unwrap();
    assert!(control.claim().is_err());
    assert_eq!(control.state(), OperationState::Active);
    control.begin_publication().unwrap();
    assert!(control.claim().is_err());
    assert_eq!(control.state(), OperationState::Publishing);
    control.committed();
    drop(guard);
    assert_eq!(control.state(), OperationState::Committed);
}

#[test]
fn cancel_and_publication_have_one_winner_and_tokens_cannot_be_reused() {
    for _ in 0..32 {
        let control = Arc::new(OperationControl::default());
        let guard = control.claim().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let other = control.clone();
        let signal = barrier.clone();
        let cancelling = std::thread::spawn(move || {
            signal.wait();
            other.cancel()
        });
        barrier.wait();
        let publication = control.begin_publication();
        if publication.is_ok() {
            control.committed();
        }
        let accepted = cancelling.join().unwrap();
        drop(guard);
        if accepted {
            assert!(matches!(publication, Err(EngineError::Cancelled)));
            assert_eq!(control.state(), OperationState::Cancelled);
        } else {
            assert!(publication.is_ok());
            assert_eq!(control.state(), OperationState::Committed);
        }
        assert!(control.claim().is_err());
    }
}
#[test]
fn failure_releases_publication_state_without_reporting_cancellation() {
    let control = Arc::new(OperationControl::default());
    let guard = control.claim().unwrap();
    control.begin_publication().unwrap();
    drop(guard);
    assert_eq!(control.state(), OperationState::Failed);
    assert!(!control.cancel());
    assert!(control.claim().is_err());
}

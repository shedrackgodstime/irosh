use super::*;
use proptest::prelude::*;

fn arb_path() -> impl Strategy<Value = String> {
    prop_oneof![Just("/".to_string()), "[a-zA-Z0-9/._-]{1,64}",]
}

fn arb_mode() -> impl Strategy<Value = Option<u32>> {
    prop::option::of(0o000u32..=0o777)
}

fn arb_failure_code() -> impl Strategy<Value = TransferFailureCode> {
    prop_oneof![
        Just(TransferFailureCode::RemoteShellUnavailable),
        Just(TransferFailureCode::TargetAlreadyExists),
        Just(TransferFailureCode::PathInvalid),
        Just(TransferFailureCode::CreateDirectoryFailed),
        Just(TransferFailureCode::SizeMismatch),
        Just(TransferFailureCode::UnexpectedFrame),
        Just(TransferFailureCode::HelperFailed),
        Just(TransferFailureCode::AtomicRenameFailed),
        Just(TransferFailureCode::Rejected),
        Just(TransferFailureCode::NotFound),
        Just(TransferFailureCode::IsDirectory),
        Just(TransferFailureCode::Internal),
    ]
}

proptest! {
    #[test]
    fn put_request_roundtrip(path in arb_path(), size in 0u64..1_000_000, mode in arb_mode(), recursive in any::<bool>()) {
        let req = PutRequest { path, size, mode, recursive };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: PutRequest = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn get_request_roundtrip(path in arb_path(), recursive in any::<bool>()) {
        let req = GetRequest { path, recursive };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: GetRequest = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn transfer_ready_roundtrip(size in 0u64..1_000_000, mode in arb_mode()) {
        let req = TransferReady { size, mode };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: TransferReady = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn entry_header_roundtrip(path in arb_path(), size in 0u64..1_000_000, mode in arb_mode(), is_dir in any::<bool>()) {
        let req = EntryHeader { path, size, mode, is_dir };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: EntryHeader = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn transfer_failure_roundtrip(code in arb_failure_code(), detail in "[a-zA-Z0-9 /._-]{0,64}") {
        let req = TransferFailure::new(code, detail);
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: TransferFailure = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn exists_response_roundtrip(exists in any::<bool>(), is_dir in any::<bool>()) {
        let req = ExistsResponse { exists, is_dir };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: ExistsResponse = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn completion_response_roundtrip(matches in prop::collection::vec("[a-zA-Z0-9/._-]{0,32}", 0..10)) {
        let req = CompletionResponse { matches };
        let json = serde_json::to_vec(&req).unwrap();
        let decoded: CompletionResponse = serde_json::from_slice(&json).unwrap();
        assert_eq!(req, decoded);
    }
}

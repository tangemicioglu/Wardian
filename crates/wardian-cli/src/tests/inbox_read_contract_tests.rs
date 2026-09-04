mod read_contract {
    use super::*;
    use std::cell::Cell;

    fn persisted_page() -> InboxProjection {
        InboxProjection {
            status_source: StatusSource::Persisted,
            items: vec![json!({"id": "disk-item"})],
            truncated: false,
            next_offset: None,
        }
    }

    #[test]
    fn absent_and_refused_endpoints_read_persisted_items_once() {
        for kind in [io::ErrorKind::NotFound, io::ErrorKind::ConnectionRefused] {
            let reads = Cell::new(0);
            let page =
                resolve_inbox_read(Err(io::Error::new(kind, "endpoint unavailable")), || {
                    reads.set(reads.get() + 1);
                    Ok(persisted_page())
                })
                .unwrap();
            let response: Value = serde_json::from_str(&render_list(&page).unwrap()).unwrap();
            assert_eq!(reads.get(), 1);
            assert_eq!(response["status_source"], "persisted");
            assert_eq!(response["items"][0]["id"], "disk-item");
        }
    }

    #[test]
    fn permission_protocol_and_transport_failures_never_read_persisted_items() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::InvalidData,
            io::ErrorKind::UnexpectedEof,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::Other,
            io::ErrorKind::TimedOut,
        ] {
            let result = resolve_inbox_read(Err(io::Error::new(kind, "live read failed")), || {
                panic!("must not read disk after {kind:?}")
            });
            let Err(error) = result else {
                panic!("live failure must propagate");
            };
            assert_eq!(error.message, "live read failed");
            if kind == io::ErrorKind::TimedOut {
                assert_eq!(error.code, "control_endpoint_timeout");
                assert_eq!(error.code_i32(), 7);
            } else {
                assert_ne!(error.code_i32(), 0);
            }
        }
    }

    #[test]
    fn backend_not_found_is_not_an_absent_endpoint() {
        let result = resolve_inbox_read(
            Err(io::Error::other(live::ControlEndpointError::new(
                "not_found",
                "requested entity missing",
            ))),
            || panic!("backend rejection must not fall back"),
        );
        let Err(error) = result else {
            panic!("backend failure must propagate");
        };
        assert_eq!(error.code, "not_found");
        assert_eq!(error.code_i32(), 2);
    }

    #[test]
    fn failed_persisted_read_is_not_replaced_with_empty_success() {
        let result = resolve_inbox_read(
            Err(io::Error::new(io::ErrorKind::NotFound, "endpoint missing")),
            || Err(CliError::generic("persisted read failed")),
        );
        let Err(error) = result else {
            panic!("persisted failure must propagate");
        };
        assert_eq!(error.message, "persisted read failed");
    }
}

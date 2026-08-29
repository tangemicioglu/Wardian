#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileResourceTicketV1 {
    pub schema: u8,
    pub ticket_id: String,
    pub url: String,
    pub resource_id: String,
    pub revision: u64,
    pub renderer_lease_id: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileResourceRangeRead {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub start: u64,
    pub end: u64,
    pub total_size: u64,
    pub partial: bool,
}

#[derive(Clone)]
struct FileReadTicket {
    issuance_id: Uuid,
    webview_label: Option<String>,
    renderer_lease: RendererLeaseKey,
    subscription_id: String,
    resource_id: String,
    snapshot: Arc<ImmutableTicketSnapshot>,
    size_bytes: u64,
    mime_type: String,
    expires_at: Instant,
}

struct ImmutableTicketSnapshot {
    file: StdMutex<File>,
    size_bytes: u64,
    reserved_bytes: u64,
    usage: Arc<AtomicU64>,
}

impl ImmutableTicketSnapshot {
    fn read_range(&self, start: u64, end: u64) -> Result<Vec<u8>, FileResourceErrorV1> {
        if start > end || end >= self.size_bytes {
            return Err(error(
                "range_not_satisfiable",
                "byte range is outside the immutable ticket snapshot",
            ));
        }
        let length = end - start + 1;
        let length: usize = length.try_into().map_err(|_| {
            error(
                "file_too_large",
                "selected byte range cannot fit in process memory",
            )
        })?;
        let mut bytes = vec![0_u8; length];
        let mut file = self
            .file
            .lock()
            .map_err(|_| error("runtime_unavailable", "ticket snapshot is unavailable"))?;
        file.seek(SeekFrom::Start(start)).map_err(|cause| {
            error(
                "runtime_unavailable",
                format!("cannot seek immutable ticket snapshot: {cause}"),
            )
        })?;
        file.read_exact(&mut bytes).map_err(|cause| {
            error(
                "runtime_unavailable",
                format!("cannot read immutable ticket snapshot: {cause}"),
            )
        })?;
        Ok(bytes)
    }
}

impl Drop for ImmutableTicketSnapshot {
    fn drop(&mut self) {
        self.usage.fetch_sub(self.reserved_bytes, Ordering::AcqRel);
    }
}

struct TicketSnapshotReservation {
    usage: Arc<AtomicU64>,
    size_bytes: u64,
    reserved_bytes: u64,
    committed: bool,
}

impl TicketSnapshotReservation {
    fn commit(mut self, file: File) -> Arc<ImmutableTicketSnapshot> {
        self.committed = true;
        Arc::new(ImmutableTicketSnapshot {
            file: StdMutex::new(file),
            size_bytes: self.size_bytes,
            reserved_bytes: self.reserved_bytes,
            usage: self.usage.clone(),
        })
    }
}

impl Drop for TicketSnapshotReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.usage.fetch_sub(self.reserved_bytes, Ordering::AcqRel);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RendererLeaseKey {
    webview_label: Option<String>,
    renderer_lease_id: String,
}

#[derive(Clone)]
struct RendererLease {
    issuance_id: Uuid,
    subscription_id: String,
    expires_at: Instant,
}

use super::*;

impl Mock {
    pub(crate) fn write_durable(
        &self,
        identity: &CacheIdentity,
        bytes: u64,
        position: u32,
    ) -> Result<(), String> {
        let sequence = identity.sequence.as_str();
        let Some(path) = self.cache_path(sequence) else {
            return Ok(());
        };
        let Some(root) = path.parent() else {
            return Err("cache path has no parent".into());
        };
        fs::create_dir_all(root).map_err(manifest::io_detail)?;
        let temporary = manifest::durable_temp_path(&path);
        let manifest = manifest::encode_manifest(identity, bytes, position);
        let mut file = fs::File::create(&temporary).map_err(manifest::io_detail)?;
        file.write_all(manifest.as_bytes())
            .map_err(manifest::io_detail)?;
        file.sync_all().map_err(manifest::io_detail)?;
        fs::rename(&temporary, path).map_err(manifest::io_detail)
    }

    pub(crate) fn read_durable(
        &self,
        sequence: &str,
        expected: Option<&CacheIdentity>,
    ) -> Result<Option<DurableState>, String> {
        let Some(path) = self.cache_path(sequence) else {
            return Ok(None);
        };
        match fs::read_to_string(path) {
            Ok(value) => manifest::decode_manifest(sequence, value.trim(), expected).map(Some),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(manifest::io_detail(error)),
        }
    }

    pub(crate) fn requires_restore(&self, sequence: &str) -> Result<bool, String> {
        if self.cache_dir.is_some() {
            return self
                .read_durable(sequence, None)
                .map(|state| state.is_some());
        }
        Ok(self
            .persisted
            .lock()
            .expect("persisted")
            .contains_key(sequence))
    }

    pub(crate) fn remove_durable(&self, sequence: &str) -> Result<bool, String> {
        let Some(path) = self.cache_path(sequence) else {
            return Ok(false);
        };
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(manifest::io_detail(error)),
        }
    }

    pub(crate) fn persist_prepared(
        &self,
        operation: &str,
        prepared: &PreparedCache,
    ) -> Result<(), String> {
        journal::write(self.cache_dir.as_deref(), operation, prepared)
    }

    /// A file-backed cache manifest is authoritative.  The in-memory map is
    /// only a process-local fixture; consulting it first would let a caller
    /// bypass the durable identity and checksum fence after the file changed.
    pub(crate) fn available_bytes(
        &self,
        cache: &p4_adapter::Cache,
    ) -> Result<Option<DurableState>, String> {
        if self.cache_dir.is_some() {
            return self.read_durable(&cache.sequence, Some(&cache_identity(cache)));
        }
        Ok(self
            .persisted
            .lock()
            .expect("persisted")
            .get(&cache.sequence)
            .copied()
            .map(|bytes| DurableState { bytes, position: 0 }))
    }
}

use super::{DedupCache, DedupDecision, DedupKey, SourceWindow};

impl DedupCache {
    pub(super) fn is_stale_boot(&self, key: DedupKey) -> bool {
        self.sources
            .iter()
            .flatten()
            .any(|source| source.node == key.src_node && key.src_boot_epoch < source.boot_epoch)
    }

    pub(super) fn source_slot(&mut self, key: DedupKey) -> Result<usize, DedupDecision> {
        if let Some(index) = self
            .sources
            .iter()
            .position(|source| source.is_some_and(|source| source.node == key.src_node))
        {
            let source = self.sources[index].expect("found");
            if key.src_boot_epoch < source.boot_epoch
                || (key.src_boot_epoch == source.boot_epoch
                    && key.request_id <= source.high_request_id)
            {
                return Err(DedupDecision::Indeterminate);
            }
            if key.src_boot_epoch > source.boot_epoch {
                self.sources[index] = Some(SourceWindow {
                    node: key.src_node,
                    boot_epoch: key.src_boot_epoch,
                    high_request_id: 0,
                });
            }
            return Ok(index);
        }
        let index = self
            .sources
            .iter()
            .position(Option::is_none)
            .ok_or(DedupDecision::Busy)?;
        self.sources[index] = Some(SourceWindow {
            node: key.src_node,
            boot_epoch: key.src_boot_epoch,
            high_request_id: 0,
        });
        Ok(index)
    }
}

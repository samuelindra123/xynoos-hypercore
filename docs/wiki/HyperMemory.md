# HyperMemory

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

**Implemented / synthetic tested:** private PageStore storage, raw/LZ4 choices, bounded accounting and transaction research. It acts only on explicitly participating process-owned buffers. It cannot transparently compress other applications or reclaim arbitrary system RAM.

Accounting is not total RSS: allocator, map metadata and temporary buffers have documented limits. CRC32 checks accidental corruption; it is not authentication. Compression can cost CPU/time or expand data. Historical cooperative experiments on PR14 remain research and did not establish desktop benefit. See [research history](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/RESEARCH-HISTORY.md) and [source](https://github.com/samuelindra123/xynoos-hypercore/blob/main/crates/xynoos-hypermemory/src/lib.rs).

# AURA Resource Intelligence

**Experimental preview.** Implemented capabilities and test evidence are described separately. Physical N5000 performance gains, complete GPU support and production readiness are **not verified**. Native APT publication is **BLOCKED** on [signing setup](https://github.com/samuelindra123/xynoos-hypercore/issues/36); Wiki publication is tracked in [#37](https://github.com/samuelindra123/xynoos-hypercore/issues/37). These maintained sources do not claim a live Wiki or APT endpoint.

**Implemented / CI fixture-tested:** deterministic read-only resource plans and memory decisions with evidence, confidence, estimated costs and limitations. Initial pressure thresholds are heuristics, not calibrated physical performance predictors.

The plan reports execution unavailable. Missing sensors reduce confidence and recommend gathering evidence. No writable power D-Bus backend, automatic optimization or production tuning service exists. Existing model/simulation policies remain separate compatible research APIs. See [resource policy](https://github.com/samuelindra123/xynoos-hypercore/blob/main/crates/xynoos-aura/src/resource.rs) and [implementation report](https://github.com/samuelindra123/xynoos-hypercore/blob/main/docs/resource-engine/https://github.com/samuelindra123/xynoos-hypercoreRT.md).

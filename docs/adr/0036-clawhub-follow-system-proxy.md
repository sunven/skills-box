# ClawHub follows the host proxy configuration

Status: accepted

The ClawHub specification's R3-Q4 required direct anonymous requests and the client explicitly disabled host proxy settings. In the current network, direct ClawHub requests take about 5.2 s each while the same requests through the existing system proxy take about 1.4 s, and direct requests occasionally fail after tens of seconds with a truncated response. Because opening a detail makes several catalog reads, the direct-only rule made ClawHub browsing and install preview visibly slow or unusable.

The ClawHub client now follows the host's system and environment proxy configuration through reqwest's `system-proxy` support, and the GitHub-backed codeload client follows the same rule. The app still adds no proxy configuration, account, token or credential of its own; it only inherits the user's existing network path. Upstream continues to receive anonymous requests, now from the proxy's egress IP instead of the device's direct IP, and the app still stores no search history. This supersedes only the direct-connection constraint in R3-Q4; the anonymous, no-account, session-cache and canonical-link rules remain unchanged. The reqwest feature also affects the other marketplace HTTP clients, none of which had a documented direct-only constraint.

An app-level per-source proxy setting was rejected for this fix because it duplicates the operating system's configuration and adds UI without a known user need. If per-source control is required later, it can extend this decision rather than replace it. Evidence and the request-waterfall changes are recorded in the [ClawHub implementation record](../../.scratch/clawhub-marketplace/implementation.md).

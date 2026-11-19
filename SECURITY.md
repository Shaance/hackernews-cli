# Security policy

Report vulnerabilities privately through GitHub. In the repository’s **Security**
tab, open **Advisories** and select **Report a vulnerability**. Keep reports out
of public issues and pull requests. [Open a private report](https://github.com/Shaance/hn/security/advisories/new).

Include the affected app and version, reproduction steps, and the impact.
Redact API keys, private story data, and provider responses.

The following areas are in scope for both apps and the shared library.

- API key storage in `~/.config/hn/summary.json`.
- Provider response logs in `~/.local/state/hn/summary-responses/`.
- Saved summaries in `~/.local/state/hn/summaries/`.
- URL validation and opening in the desktop article pane or an external browser.

# mu-cli

Terminal client for [wearmu.com](https://wearmu.com) — make MU products.

```
pip install -e .
export MU_AGENT_KEY=...   # or ~/.mu/secrets / ./.secrets.local
mu --ai "a minimal black sumi-e crescent moon on pure white" --kind tee
mu-batch briefs.json --workers 6
```

Agents should use the MCP server (https://mcp.wearmu.com) instead.
Generate a client for any language from https://wearmu.com/openapi.json.

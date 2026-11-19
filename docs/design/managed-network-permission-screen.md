# Proposed combined-app permission screen

Show this English scheme before implementing the combined-app GUI. The current
standalone controls do not grant managed members. The backend proposal is
[managed grants](managed-network-permissions-proposal.md), not implemented yet.

```text
Network access — Example app

Applies to these 2 current services:
  web
  db

Internet                 [Off] [Allow for listed services]
Private networks (LAN)   [Off] [Allow for listed services]

New or replaced services start with access off.
Permissions apply the next time you start this app.

[Cancel]                  [Save for these 2 services]
```

Both categories are independent. This explicitly reviews all listed current
services; it does not grant future members or let a recipe choose consent. If
current flags differ across services, show Some services and leave that category
unselected until the user chooses Off or Allow for listed services. Save requires
an explicit resolution of every mixed category; it must not silently turn Mixed
into either a grant or withdrawal. Cancel preserves current rows.

The existing background-service/browser limitation, same-network DNS disclosure
and Android/VPN routing helper remain visible. Running/data/definition changes,
stale owner/review or unsupported build disable save and require current review.
Source lists only bounded actual service names/count/current permission summary,
never args/environment/commands/raw plan or caller-chosen member identity.

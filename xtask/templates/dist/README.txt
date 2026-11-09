Flyff V15 server {{VERSION}}
================================

One static Linux executable ("flyff") that contains the game server, the web server and the browser
client. No installation, no database server, no other runtime is needed. You play in a browser (Chrome,
Edge or Firefox on a desktop); there is no other client.

What is in this folder
----------------------
  flyff                 the server (static x86-64 Linux executable)
  Resource/             the ORIGINAL Flyff V15 data files (only if this folder was packaged with them)
  flyff.toml.example    all settings with their defaults (the server writes its own flyff.toml on first start)
  flyff.service.example systemd unit
  scripts/npc/          NPC dialog scripts (editable Lua; MANIFEST.txt lists the shipped checksums)
  docs/                 admin-console.md: the operator guide of the admin console (with screenshots)
  VERSION               build id, protocol hash, target
  LICENSES/             third-party licences of the components inside the executable

The original game data (Resource/) is proprietary and is NOT part of the executable: put your own copy
of the "Resource" folder (it contains Masquerade.prj) next to "flyff", or start with --resource /path/to/Resource.
The server never writes into Resource/. Files you put into overrides/ shadow files of Resource/.

Quick start
-----------
  1. Put the "Resource" folder next to "flyff" (copy it, or symlink it).
  2. ./flyff check          (optional) validates the configuration and the data and prints the counts
  3. ./flyff                starts the server (same as ./flyff serve)
  4. Open the printed address, for example http://192.168.1.20:8080/ , in your browser.

First start
-----------
The server creates, next to the executable: flyff.toml (every setting, commented), data/ (the database),
cache/ (derived assets, deletable), logs/ and backups/. It loads the game data (about a second), builds the
server-side tables (about a second on the first start, a fraction of that afterwards) and prints
"Open http://...". Everything else that the browser needs (converted images, compressed copies) is
built in the background at low priority and never delays the first player; if a browser asks for something
that is still being built, the client shows "preparing assets".

It also creates the administrator account "admin" with a random password. The password is printed ONCE on
the console and kept in logs/first-start.txt (readable by the owner only). Other players create their
accounts on the login screen while auth.allow_registration is true (the default).

Useful commands
---------------
  ./flyff check                    validate config, Resource, data, caches, scripts/npc and the database
  ./flyff check --expect-reference compare the data counts with the original repack (exit 1 on a difference)
  ./flyff prepare                  build all derived assets now (about 10 s on 16 cores) instead of in the
                                   background; may run while the server runs
  ./flyff prepare --kind png --world wdmadrigal --threads 4     only some kinds / worlds
  ./flyff version                  build id and protocol hash
  ./flyff config default           print the commented default configuration
  ./flyff config show --changed    show non-default settings and where they come from
  ./flyff db migrate --dry-run     list pending schema migrations
  ./flyff db backup                write backups/flyff-<date>-manual.db now (works while the server runs)
  ./flyff db check [--full]        check the database (also --serials for duplicated items, --items); safe while running
  ./flyff db restore <FILE>        put a backup back (server stopped; the current database is saved first)
  ./flyff db vacuum                shrink the database file (server stopped; a backup is written first)
  ./flyff character undelete NAME  bring back a deleted character (server stopped; before it is purged)
  ./flyff --home /srv/flyff        use another runtime folder (also FLYFF_HOME)
  ./flyff --bind 127.0.0.1:9000    listen on another address (also FLYFF__NET__BIND)

Start-up time: the server is ready in a few seconds from an empty cache (target 15 s) and in about a
second with a warm cache (target 5 s). If a slow disk makes the first start too slow, run "./flyff prepare" once.
Exit codes: 0 ok, 1 error, 2 data error (a file of Resource/ is missing or broken), 64 usage, 66 Resource
folder missing, 70 internal error, 74 database error, 75 another instance is running, 78 invalid configuration.

Network
-------
The server listens on TCP port 8080 (setting net.bind). Plain HTTP on a private network (LAN) is the
supported way to play. Anyone who can reach the port can download the game assets (web.public_assets), so keep
the server on a LAN or a private host, or put it behind TLS (a reverse proxy; set web.trusted_proxies) before
exposing it to the internet. On start the server warns when it is reachable on a public address.
Open TCP 8080 in your firewall for other machines. /metrics (Prometheus) is reachable from the server machine
itself; remote access needs admin.allow_remote = true and the token in data/admin.token (Authorization: Bearer).

Admin console
-------------
Open http://127.0.0.1:8080/admin in a browser ON THE SERVER MACHINE: the machine itself is the owner and gets the whole
console without a login (live status, players, items, moderation, the server settings). Nothing is reachable from other
computers until you decide so.
  - Staff sign in with their game account (authority L or higher) from the account menu or the sign-in page and act with
    their own letter (L observe, M moderate, N game master, P administrator); every action is recorded under the account's
    name. Make staff with "./flyff account set-auth NAME M". The first sign-in of the administrator account "admin" asks for a
    new password (the one-time password of the first start is in logs/first-start.txt); the game login keeps working.
  - Other computers: set admin.allow_remote = true in flyff.toml and put a TLS reverse proxy in front (set
    web.trusted_proxies), or leave it off and use an SSH tunnel: "ssh -L 8080:127.0.0.1:8080 you@server", then open
    http://127.0.0.1:8080/admin on your own computer. Plain HTTP from another computer works but the console shows a red
    warning: its session cookie travels in the clear.
  - data/admin.token is the owner's secret (Authorization: Bearer for tools; "Use the owner token" on the sign-in page).
    Keep it private. Set admin.owner_loopback = false to make even the server machine sign in with a staff account.
  - If the console does not load, open http://127.0.0.1:8080/admin/lite: a small rescue page with status, the online list,
    kick, a notice, a shutdown countdown, rates, events, accounts and the console.
The guide docs/admin-console.md walks through every part; each category of the console has a "?" button that explains it.

Backups and upgrading
---------------------
data/flyff.db holds every account and character. The server backs it up by itself into backups/ every hour (the
last 24 hours, 7 days and 4 weeks are kept; see the [backup] section of flyff.toml, which can also run a hook to
copy the files to another machine). "./flyff db backup" makes one more at any time, even while the server runs. Do
not copy data/flyff.db itself while the server runs.

Stopping: Ctrl-C, SIGTERM or "shutdown [seconds] [message]" on the console. The players see a countdown (60, 30,
10, 5, 4, 3, 2, 1 s), then everybody is logged out with a save, the database is checkpointed and the process exits
0. A second Ctrl-C during the countdown stops at once. "shutdown cancel" cancels it. The browser goes back to the
login screen.

To upgrade: stop the server, replace the "flyff" executable, keep flyff.toml, data/ and Resource/, and start it
again. Schema migrations run automatically; the previous database is saved to backups/ first. To go back to an
older version, stop the server and run "./flyff db restore backups/flyff-<date>-premigrate.db" with the old
executable.

systemd
-------
See flyff.service.example (copy it to /etc/systemd/system/flyff.service and adjust the paths and the user).
"systemctl reload flyff" (SIGHUP) re-reads flyff.toml without a restart, like the console word "reload": the keys
marked "reload" in flyff.toml.example and the [rates] and [events] values take effect at once, every other changed
key is listed as "restart required" in the log and waits for the next start.

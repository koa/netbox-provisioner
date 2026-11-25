# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsichten des Codes vom 2026-10-19 und 2026-11-06. Die Nummern bleiben, auch wenn umsortiert wird; eine
erledigte Aufgabe wandert mit Datum und Commit nach [done.md](done.md), ein erledigter Teil
ebenso, der Rest bleibt hier.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**.

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“); sind alle Aufgaben einer
Entscheidung erledigt, wandert sie nach [done.md](done.md#entscheidungen).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z4 | Freigaben der internen Firewall: Custom Fields am Interface oder Prefix-Tags? | Custom Fields am Interface (2026-10-23); geändert (2026-11-23): Tags `guest`/`protected` und Custom Fields am Prefix, das Gateway findet der Generator, die strengere Seite von IPv4 und IPv6 gewinnt, nicht vererbt | C3 |
| Z10 | Anwohner als Tenant oder Contact Group, und wo stehen seine Benutzer? | weder noch: ein NetBox-Owner mit seinen NetBox-Benutzern in `users` (Login über Pocket-ID), in der Pflicht-Owner-Group „Anwohner“; Mitglieder werden in NetBox gepflegt, damit dieselbe Zuordnung die Leserechte in NetBox steuert (in der Lab-NetBox geprüft, 2026-10-23) | C7 |
| Z11 | Ports eines Anwohners an einem fremden Switch? | NetBox-Feld `owner` am Interface, kein Custom Field (2026-10-23) | C7 |
| Z12 | Apply eines Anwohners sofort oder mit Admin-Freigabe? | sofort (2026-10-23); abgelöst durch Z46: der Anwohner schreibt nur NetBox, die Geräte erreicht es über das automatische Rollout | C7 |
| Z13 | Alarme im Provisioner oder über Prometheus? | nur Prometheus/Alertmanager (2026-10-23) | C8 |
| Z14 | Ziel des Uplink-Pings? | das Gateway des Uplinks (2026-10-23) | C8 |
| Z16 | Grenzwerte für Health? | keine: Health nicht beurteilen, nur anzeigen (Hover/Popup); Farben nur nach Erreichbarkeit (2026-10-23) | C8, C9 |
| Z39 | Reiner L2-Switch (C18): Wie wird er in NetBox modelliert, und welche VLANs trägt sein Trunk? | Custom Field `l3_uplink` am Port des alten Switches zum L3-Switch, dessen L3-Funktionen dort umgesetzt werden; VLAN-IDs automatisch aus einem auf beiden Switches unbenutzten Bereich (2026-11-07) | C18 |
| Z41 | Zertifikat eines OpenVPN-Servers (E26, nur wo OpenVPN nach C22 bleibt): Woher weiss der Provisioner, dass ein Gerät einen OpenVPN-Server hat (ein Service in NetBox, ein Custom Field am Gerät)? Stellt `www_ca` auch dieses Zertifikat aus, oder eine eigene CA für OpenVPN (die Clients sollten nur ihr trauen)? | **offen** | E26 |
| Z42 | Was bekommt ein Anwohner, und woher weiss man, welche WLANs und welche WLAN-Gruppe ihm gehören? | Geräte, Ports fremder Geräte und VLANs (`owner`); die WLANs auf seinen VLANs gehören ihm, seine WLAN-Gruppe ist die seiner WLANs (2026-11-17; Bedingung `vlan__owner__users` in der Lab-NetBox geprüft) | C7 |
| Z43 | Eine WLAN-Gruppe pro Anwohner oder alle APs einer Gruppe mit allen WLANs? | alle APs strahlen alle WLANs der Gruppe aus (2026-11-17) | C7 |
| Z44 | Wie wird ein fremdes Gerät am Port eines Anwohners kein OSPF-Nachbar? | OSPF-Authentisierung pro Link (2026-11-17) | A10, C7 |
| Z45 | Fremde VLANs von einem Gerät des Anwohners fernhalten? | nein, er konfiguriert es nur über den Provisioner (2026-11-17) | C7 |
| Z46 | Dürfen Anwohner-Änderungen fremde Geräte betreffen? | ja; die Mutationen regeln nur, was er in NetBox eintragen darf, auf die Geräte kommt es über ein automatisches Rollout (2026-11-17) | C7, C20 |
| Z47 | Darf ein Anwohner WLANs anlegen? | nein, vorerst nur Ports und bestehende WLANs: Passwort, nach dem Prototyp auch SSID (eindeutig in der WLAN-Gruppe) und ein/aus (2026-11-17) | C7, C21 |
| Z48 | Wer richtet ein Gerät eines Anwohners ein? | vorerst der Admin (2026-11-17) | C7 |
| Z49 | Was ist ein Netz für den Anwohner? | ein VLAN mit höchstens einem WLAN, das er ein- und ausschaltet (2026-11-17) | C21, E27 |
| Z50 | Anschluss (Uplink-Port) eines Geräts automatisch erkennen? | ja, über die Nachbarn; übernommen wird mit einem Klick, nicht von selbst (2026-11-17) | C21 |
| Z51 | Rückgängig nach gescheitertem Ausrollen? | nein, es bleibt als Abweichung sichtbar; eine ausgerollte Änderung lässt sich als neue Änderung zurücknehmen (2026-11-17) | C20, C21 |
| Z52 | Darf ein Admin das UI eines Anwohners sehen und dort ändern? | ja, Support-Ansicht mit Auswahl des Anwohners; der Verlauf nennt beide; später vielleicht Stufen (nur sehen) (2026-11-17) | C21 |
| Z59 | VPNs in NetBox (C22): das eigene VPN-Modell oder etwas anderes? | das eigene VPN-Modell, ein Tunnel pro Paar von Enden, Terminations an virtuellen Interfaces, Custom Field `endpoint` (DDNS-Name) an der Termination; IPv6/IPsec/GRE zwischen den Hubs (`gre` mit IPsec-Profil), IPv4/WireGuard vom Spoke zum Hub (2026-11-25) | C22 |
| Z60 | Gegenstelle in einem Netz ausserhalb NetBox (C22): ein Gerät dafür in NetBox (eigene Site „extern“, nur Tunnel-Interface und äussere IP), oder der Tunnel mit einer Termination und die Gegenstelle in der Beschreibung? | **offen** | C22 |
| Z61 | BGP mit den VPNs in NetBox (C22)? Am 2026-10-02 entschieden: bleibt von Hand, ohne NetBox-Modell; mit den VPNs neu zu entscheiden | **offen** | C22 |
| Z62 | Erzeugt der Provisioner die WireGuard-Tunnel aus NetBox (Schlüssel abgeleitet wie `ospf_auth`) oder dokumentiert NetBox sie nur und der Provisioner vergleicht? | **offen** | C22 |
| Z63 | Hubs der VPNs (C22)? | die beiden Standorte mit statischem IPv6-/48 und 10 Gbit/s (CCR2004, RB5009), untereinander über IPv6 mit IPsec/GRE; die übrigen fünf (RB1100AHx4, RB760iGS, drei Chateau an LTE hinter CGNAT) über IPv4 mit WireGuard zu beiden (2026-11-25) | C22 |

## A. Sicherheit

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C3 | Interne Firewall: `guest` und `protected` am Prefix, Freigaben als Custom Fields am Prefix (Z4), Regeln am Gateway-Interface, das der Generator findet; nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C7 | Self-Service und Personas (gebaut: Besitz, Rollen, lesende Schnittstelle, WLAN: SSID, Passwort, ein/aus, Netz an Port und Dose, Leases und WLAN-Clients eines Netzes mit ihrem Band, eigene Geräte anschliessen und abhängen, ihr Anschluss (auch der erkannte) und Raum, mehrere Änderungen in einem Plan, auf die Geräte über das automatische Rollout (C20, Z46), [self-service.md](self-service.md)); NetBox und Provisioner über Pocket-ID mit der Gruppe „Anwohner“ und den Claims im Lab geprüft (2026-11-25, [login.md](login.md#pocket-id)); offen: Leserechte in der produktiven NetBox nur auf eigene WLANs (von Hand, wie `local/netbox/seed-residents.py`); Ersteinrichtung bleibt beim Admin (Z48) | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C18 | Reiner L2-Switch als „Extender“: Ein alter Switch ohne L3-Offload oder mit anderen Problemen (CRS2xx, CRS354) hängt an einem modernen Switch oder Router, der seine L3-Funktionen über einen Trunk übernimmt: Port oder Bridge, mit IP oder VLAN (Z39) | [Reiner L2-Switch](concepts/l2-extender.md) | M–L | offen |
| C22 | VPNs: OpenVPN- und IPsec-Verbindungen zwischen den Netzen (Routing über BGP), auch zu einem Netz ausserhalb NetBox, lesend erfassen, in NetBox dokumentieren und wo möglich auf WireGuard umstellen; sieben Standorte: alle IPv4 dynamisch, drei davon Mobilfunk hinter CGNAT, zwei mit statischem IPv6-/48 (Z59–Z63) | [VPNs](concepts/vpn.md) | L | offen |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|

## E. Erweiterungen umgesetzter Teile

| # | Aufgabe | Doku | Aufwand | Status |
|---|---|---|---|---|
| E3 | Das Image in einem Cluster ausrollen und die beiden Workflows (CI, Aufräumen) auf GitHub einmal laufen lassen | [Deployment](deployment.md#noch-nicht-geprüft) | S | offen |
| E8 | Weitere Befunde (Rest; erledigt: WLAN-Gruppe ohne Controller, Land, OSPF-Nachbar an einem Port ohne `ospf`, Board gegen Device Type): die Fehler des Generators gesammelt pro Site und PoE als Befund für alle Geräte einer Site | [Config-Helper](config-helper.md#befunde) | M | offen |
| E9 | Automatischer Test der Helfer gegen das Lab-NetBox: ausführen, Vorschau mit dem Ergebnis vergleichen, zurücknehmen | [Config-Helper](config-helper.md#technik) | M | offen |
| E10 | Rollout im Feld: ein kurzer Test „Strecke ok“ (Ping, Bandbreite zum Nachbarn) und eine Ansicht der Strecke | [Geräteseite](device-page.md) | M | offen |
| E12 | Ansicht „Was fehlt diesem Gerät noch“: die Befunde des Geräts mit den Helfern als Knopf, der Reihe nach durch Loopback, Verbindung usw., ohne Wizard zu sein | [Geräteseite](device-page.md) | M | offen |
| E13 | Diff in Worten neben dem Skript („entfernt 2 Adressen, ändert die SSID“), die heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports) | [Geräteseite](device-page.md) | M | offen |
| E15 | Port angeschlossener Geräte ohne RouterOS erkennen: Für ein Gerät mit dokumentierter IPv4 oder IPv6 über ARP, IPv6-Nachbarn und die Host-Tabelle der Bridge finden, an welchem Port des Routers oder Switches es hängt, und in der Live-Prüfung der Interfaces ein fehlendes oder anders gelegtes Kabel melden, mit einem Helfer, der es in NetBox anlegt (Plan, Vorschau, Rückgängig). Zuordnung über die IP, nie über MACs aus NetBox; mehrere MACs an einem Port (Switch ohne RouterOS dahinter) machen ihn mehrdeutig | [Config-Helper](config-helper.md#netbox-gegen-gerät) | M–L | offen |
| E23 | Umbenennung von Geräten: Die Gerätenamen sind heute unstrukturiert, einige werden sich ändern. Nach einer Umbenennung in NetBox soll man im Provisioner den neuen Namen dem alten zuweisen können, damit die Gerätekonfiguration nachgezogen wird (heute zeigt die Geräteseite nur, dass `/system/identity` nicht der NetBox-Name ist; heisst das Gerät wie ein anderes NetBox-Gerät, gilt es als `WrongDevice`) | [Geräte umbenennen](concepts/device-rename.md) (Entwurf) | M | offen |
| E25 | Fehler beim Start des Apply-Skripts zeigen: Lehnt RouterOS das Skript schon beim Parsen ab (lab-crs125, 7.24.6: `expected end of command (line 61 column 5)` im Log, Topic `script,error`), setzt es keine Marke; der Job endet mit „Nicht alles wurde angewendet“ und dem ganzen Restskript, aber ohne den Grund. Fehlt nach `unmarked` jede Marke, die Meldung aus `/log` lesen und mit Zeile und Befehl beim Job zeigen | [Jobs](jobs.md) | S | offen |
| E26 | Zertifikat für einen OpenVPN-Server: das Zertifikat aus einer CA ausstellen, hochladen und belassen wie das von WebFig (`www_certificate`), wo NetBox einen OpenVPN-Server nennt (Z41). Dass Apply eine OpenVPN-CA entfernte, ist behoben (in done.md). Nur wo OpenVPN nach C22 bleibt | [Deployment](deployment.md) | M | offen |
| E30 | Wartefenster nach einem Apply, das einen OSPF-Link umnummeriert: Das Fenster nach dem Skript (`apply_fallback_seconds`, 90 s) reicht nicht, wenn sich die Adresse oder das Template eines `ospf`-Ports ändert. Im Lab (lab-np2 – lab-wap, 2026-11-20, ein Anwohner hängte lab-wap ab und wieder an) war lab-wap danach 53–54 s nicht erreichbar, IPv4 kam später als IPv6; beim zweiten Versuch hatte lab-wap bei der letzten Prüfung vor dem Rückfall IPv6, aber kein IPv4, und beide gingen zurück; mit 240 s lief derselbe Batch durch. Ein dritter (Anschluss übernommen, neue /31 und neue OSPF-Schlüssel) lief in 67 s durch: Es trifft nicht jedes Mal. Ob die neue Adresse oder der neue Kommentar am Template das OSPF-Interface neu startet (Wartezeit bis zur DR-Wahl im Broadcast-Netz), ist ungeprüft. Plan: auf einer CHR klären, was neu startet; dann wie `radio_wait` ein längeres Fenster für einen Plan, der das tut, oder den Neustart vermeiden | [Apply](config-helper.md) | M | offen |
| E33 | Geräte in der Vorschau eines Anwohners genau: `ResidentPlan.devices` nähert an (Geräte der Schritte, APs einer WLAN-Gruppe, die Geräte eines VLANs mit VXLAN, wenn ein Port-Gerät es betritt oder verlässt). Ein Switch ohne RouterOS hinter einer Dose ist kein VTEP, nennt so aber alle Geräte des VLANs; geändert hat sich im Lab nur lab-np2 (die Bridge-VLAN zum Switch, 2026-11-21), ein VLAN ohne VXLAN nennt so niemanden auf dem Weg. Plan: die Konfiguration, die der Generator aus NetBox vor und nach dem Plan für die RouterOS-Geräte der betroffenen VLANs erzeugt, vergleichen (ohne Lesen am Gerät) und nur die mit Unterschied nennen | [Self-Service](self-service.md) | M | offen |
| E35 | Seit wann ein Link fehlt: Der Anwohner-Entwurf sagt „Kein Link seit 09:15“, `PortLive.link` kennt nur jetzt; den Zeitpunkt des Wechsels festhalten (aus `last-link-down-time` des Ports oder beim Lesen gemerkt) und im Text nennen | [Self-Service](concepts/self-service.md#anwohner-ui) | S | offen |
| E36 | WLAN-Clients, die mit einem Access Point verschwinden: Fällt ein AP eines WLANs aus, sieht der Anwohner das WLAN „auf einem Teil seiner Access Points“, aber nicht, welche Geräte dabei aus „Geräte im Netz“ wegfielen; zuletzt gesehene Clients pro AP merken und als Meldung zeigen | [Self-Service](concepts/self-service.md#anwohner-ui) | M | offen |
| E38 | Anwohner-UI mit dem Entwurf vergleichen (`docs/concepts/self-service-ui.dc.html`): Der Vergleich durch den Benutzer steht aus; was er findet, wird zu eigenen Aufgaben | [Self-Service](concepts/self-service.md#anwohner-ui) | S | offen |

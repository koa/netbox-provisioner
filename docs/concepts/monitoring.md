# Konzept: Überwachung

Status: Konzept, zum Teil umgesetzt: Health, PoE, WLAN-Clients, System, Interfaces, Leases, die
Strecken aus der LSDB und den Uplink-Ping liefert der Provisioner an `/metrics`
([Gerätemetriken](../device-metrics.md), C14, C8); die Seite „Zustand“ für Admins gibt es
(`/condition`, siehe unten), ebenso die Internet-Zeile der Anwohner. Offen sind die Radios von
`/interface/wifi` und die 60-GHz-Strecken. Der Rest ist
Aufgabe C8 in [todo.md](../todo.md). Baut auf der [Netz-Karte](../network-map.md) auf.

## Ziel

„Jeder Anwender soll auf den ersten Blick sehen können, wie gut das Netz läuft“, auf zwei Ebenen:

- **Für sich:** Wie gut ist meine Internetverbindung (Anwohner, siehe
  [Self-Service](self-service.md))?
- **Für alle:** Was muss kontrolliert oder gewartet werden (Admin)?

## Ausgangslage

- Der Provisioner liest Geräte, wenn jemand eine Seite öffnet, ein Job läuft, Prometheus
  `/metrics` abfragt oder eine Runde des automatischen Rollouts die Diffs liest.
- `/metrics` am Management-Port hat die Gerätemetriken ([Gerätemetriken](../device-metrics.md)):
  System, Health, PoE, Interfaces und Ethernet-Links, Funk-Radios mit Frequenz und
  Rauschen, WLAN-Clients mit Signal, gebundene Leases. Verlauf und Grafana-Dashboards gibt es
  damit schon.
- Die [Netz-Karte](../network-map.md) beschreibt Ist-Zustand aus der OSPF-LSDB eines Routers (ca.
  alle 5 s), Farben für Knoten und Kanten, Health (Spannung, Temperatur) und PoE.
- Aus der Konfiguration kennt der Provisioner Sollwerte: OSPF-Nachbarn aus NetBox, Watchdogs,
  PoE-Beziehungen, Funkstrecken.

## Vorschlag

**Ein Sammler im Backend**, der periodisch liest und den letzten Zustand hält. Health, Uptime,
die WLAN-Clients und das Signal der 5-GHz-Funkstrecken (beim AP-Ende) liefert `/metrics` schon,
pro Scrape gelesen statt vom Sammler; ebenso die Strecken aus der LSDB (`mikrotik_link_up`,
`mikrotik_link_ipv6_up`) und der Uplink-Ping (`mikrotik_uplink_ping_*`, 2026-11-25): Ein eigener
Sammler ist damit nicht nötig, Prometheus fragt regelmässig. Offen sind die 60-GHz-Strecken
(`w60g` monitor) und die Radios von `/interface/wifi` in `mikrotik_wireless_radio_*`:

| Was | woher | wie oft |
|---|---|---|
| Strecken und Router | OSPF-LSDB eines Routers ([Netz-Karte](../network-map.md)) | 5 s |
| Funk: Signal, CCQ/MCS, Rate | `/interface/wireless/registration-table`, `w60g` monitor, auf AP-Seite | 60 s |
| Health: Spannung, Temperatur | `/system/health` | 5 min |
| Uplink-Qualität: Verlust und Latenz zum Gateway des Uplinks | `/ping` vom `internet`-Router auf sein Default-Gateway | 60 s |
| Uptime, Neustarts (Watchdog ausgelöst?) | `/system/resource` | 5 min |

Das liest nur. Es fasst keine Konfiguration an und läuft nicht als Job.

**Ausgabe:**

- **Prometheus:** Alles auch als Metriken an `/metrics` (`device`, `site`, `link` als Labels).
  Für Verlauf und Alarme reicht dann ein vorhandenes Grafana/Alertmanager; der Provisioner
  speichert keine Historie.
- **UI, Admin:** eine Seite „Zustand“ mit den Befunden in Reihenfolge der Dringlichkeit: Router
  unerreichbar, Strecke tot, Strecke schlecht (Signal, Verlust), Neustart in den letzten 24 h,
  PoE-Port ohne Last. Health (Spannung, Temperatur) wird angezeigt, aber nicht beurteilt; wer
  Grenzwerte will, setzt sie als Alarm in Prometheus. Jeder Eintrag verlinkt das Gerät. Die Karte zeigt
  dasselbe räumlich.

  **Umgesetzt (2026-11-25):** `/condition` „Zustand“ im Site-Menü der Breadcrumb, für alle Sites.
  Befunde in dieser Reihenfolge: Router, den der Provisioner nicht erreicht (seine Strecken, die
  nur seinetwegen unten sind, nicht noch einmal); Strecke unten in OSPFv2; Strecke nur in OSPFv2
  oben (OSPFv3 fehlt, IPv6 aus?); Router mit Uplink (Port mit Tag `internet`, oder er verteilt die Default-Route) ohne aktive Default-Route; Uplink mit
  Verlust zum Gateway (oder ohne Antwort); Neustart in den letzten 24 h; PoE-Port mit `pse` in
  NetBox, an dem nichts Strom zieht. Darunter alle Router, die geantwortet haben, mit Ressourcen
  und Health wie in der Karte. Gelesen beim Öffnen und mit „Neu lesen“, nicht laufend: Die Seite
  liest jeden Router (im Lab mit 7 Routern 2,5 s); was die Netz-Karte vor höchstens 30 s gelesen
  hat, nimmt sie von dort (die OSPF-Datenbank höchstens 5 s alt). Dazu (Z64, 2026-11-26) jeder WLAN-Client
  mit schwachem Signal, wie ihn sein AP hört (bei CAPsMAN der Controller): unter `wlan_signal.weak`
  (−75 dBm) eine Warnung, unter `wlan_signal.bad` (−80 dBm) ein Fehler, mit SSID, AP und dem
  Namen aus einer Lease, sonst der MAC; auch die Gegenstelle einer Funkstrecke. Das Gesamtsignal,
  nicht die schwächste Antenne; kein CCQ, kein SNR (RouterOS' Wert passt nicht zum thermischen
  Rauschen, lab-sxt-a: −69 dBm und 41 dB ergäben −110 dBm Rauschen bei 80 MHz).
- **UI, Anwohner:** eine Zeile pro eigenem Netz: „Internet: gut (12 ms, 0 % Verlust)“, und der
  Weg dorthin als Kette der Strecken mit ihrem Zustand. Gerechnet über den Baum aus
  `topology::management`: der Weg vom Router des Netzes zum NAT-Router.

  **Umgesetzt (2026-11-25):** in der Karte jedes Netzes. Der Weg beginnt beim Router mit dem
  DHCP-Server des Netzes und folgt dem Management-Baum bis zum ersten Router mit Uplink (Port mit
  Tag `internet`, oder er verteilt die Default-Route; sonst bis zur Wurzel). Jeder Router darauf
  ist erreicht oder nicht (aus der LSDB, wie die übrigen Live-Zustände): Statt der Strecken
  zeigt die Kette die Router, denn zwischen zwei Routern im Baum liegt nicht immer ein Kabel
  (ein Switch auf L2). „Internet: unterbrochen, nicht erreicht: …“, wenn einer fehlt; sonst
  entscheiden die Pings des Uplink-Routers (höchstens 60 s alt): „gut“ ohne Verlust, „gestört“
  mit, „Das Gateway des Uplinks antwortet nicht“, „unterbrochen“ ohne Default-Route am Uplink;
  „nicht gemessen“ ohne Uplink auf dem Weg (im Lab: lab-np2 ist die Wurzel, ohne Uplink in
  NetBox), wenn der Uplink-Router nicht antwortet oder seine Default-Route über ein Interface
  ohne Adresse des Gateways geht (PPPoE, LTE). Die Anwohner sehen die Adresse des Gateways
  nicht (`ResidentUplink`).

## Grenzen

- Ein toter Router kann nicht sagen, was ihm fehlt; sichtbar ist nur, dass er weg ist und wie
  lange.
- Last auf schwachen Geräten: Abfragen bündeln, nicht jede Sekunde; die LSDB von einem Router
  statt alle einzeln.
- Der Provisioner wird damit zu einem Dienst, der immer läuft und alle Geräte erreicht. Das ist
  er heute schon für die UI, aber nicht dauerhaft unter Last.

## Reihenfolge

1. Sammler mit LSDB und Prometheus-Metriken (Grundlage der Netz-Karte).
2. Seite „Zustand“ für Admins.
3. Funk, Health, Uplink-Ping (Uplink-Ping erledigt; Health nur gezeigt, Z16).
4. Anwohner-Ansicht im [Anwohner-UI](../self-service.md#anwohner-ui) (erledigt).

## Historie über Syslog

Eine Idee aus der [Netz-Ansicht](../network-map.md), nicht geplant: Alle Router schicken ihre
OSPF-Meldungen per Syslog (`/system/logging action` mit `remote`, Topic `ospf`) an den
Provisioner. Vorteil: Ereignisse im Moment, in dem sie passieren, und eine Historie
(„Funkbrücke 3-mal in der letzten Stunde weggefallen“). Nachteile: Syslog über UDP geht genau
dann verloren, wenn die Strecke dazwischen ausfällt, und den aktuellen Zustand muss man
trotzdem abfragen. Die Netz-Ansicht fragt deshalb ab. Das Logging der Geräte an einen Server
(`device_syslog`) generiert der Provisioner schon; ob er selbst einer wird, ist offen.

## Entschieden (2026-10-23)

- Alarme **nur über Prometheus/Alertmanager**, keine im Provisioner.
- Der Uplink-Ping geht an das **Gateway des Uplinks**, kein konfiguriertes Ziel.
- Health wird nicht beurteilt, nur angezeigt (wie in der [Netz-Karte](../network-map.md)).

# Konzept: Überwachung

Status: Konzept, nicht umgesetzt. User Journey „Überwachung“ aus
[pending.md](../pending.md#überwachung). Baut auf der [Netz-Karte](network-map.md) auf.

## Ziel

„Jeder Anwender soll auf den ersten Blick sehen können, wie gut das Netz läuft“, auf zwei Ebenen:

- **Für sich:** Wie gut ist meine Internetverbindung (Anwohner, siehe
  [Self-Service](self-service.md))?
- **Für alle:** Was muss kontrolliert oder gewartet werden (Admin)?

## Ausgangslage

- Der Provisioner liest Geräte nur, wenn jemand eine Seite öffnet oder einen Job startet.
  Dauerhaft fragt er nichts ab.
- Er hat schon `/metrics` (Prometheus) am Management-Port, heute nur mit HTTP-Metriken.
- Die [Netz-Karte](network-map.md) beschreibt Ist-Zustand aus der OSPF-LSDB eines Routers (ca.
  alle 5 s), Farben für Knoten und Kanten, Health (Spannung, Temperatur) und PoE.
- Aus der Konfiguration kennt der Provisioner Sollwerte: OSPF-Nachbarn aus NetBox, Watchdogs,
  PoE-Beziehungen, Funkstrecken.

## Vorschlag

**Ein Sammler im Backend**, der periodisch liest und den letzten Zustand hält:

| Was | woher | wie oft |
|---|---|---|
| Strecken und Router | OSPF-LSDB eines Routers ([Netz-Karte](network-map.md)) | 5 s |
| Funk: Signal, CCQ/MCS, Rate | `/interface/wireless/registration-table`, `w60g` monitor, auf AP-Seite | 60 s |
| Health: Spannung, Temperatur | `/system/health` | 5 min |
| Uplink-Qualität: Verlust und Latenz zu einem Ziel im Internet | `/ping` vom NAT-Router, oder `/tool netwatch` dort | 60 s |
| Uptime, Neustarts (Watchdog ausgelöst?) | `/system/resource` | 5 min |

Das liest nur. Es fasst keine Konfiguration an und läuft nicht als Job.

**Ausgabe:**

- **Prometheus:** Alles auch als Metriken an `/metrics` (`device`, `site`, `link` als Labels).
  Für Verlauf und Alarme reicht dann ein vorhandenes Grafana/Alertmanager; der Provisioner
  speichert keine Historie.
- **UI, Admin:** eine Seite „Zustand“ mit den Befunden in Reihenfolge der Dringlichkeit: Router
  unerreichbar, Strecke tot, Strecke schlecht (Signal, Verlust), Neustart in den letzten 24 h,
  Temperatur hoch, PoE-Port ohne Last. Jeder Eintrag verlinkt das Gerät. Die Karte zeigt
  dasselbe räumlich.
- **UI, Anwohner:** eine Zeile pro eigenem Netz: „Internet: gut (12 ms, 0 % Verlust)“, und der
  Weg dorthin als Kette der Strecken mit ihrem Zustand. Gerechnet über den Baum aus
  `topology::management`: der Weg vom Router des Netzes zum NAT-Router.

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
3. Funk, Health, Uplink-Ping.
4. Anwohner-Ansicht, sobald es [Self-Service](self-service.md) gibt.

## Zu entscheiden

- Alarme im Provisioner selbst oder nur über Prometheus/Alertmanager? Vorschlag: nur Prometheus.
- Ziel des Uplink-Pings (je NAT-Router eins, als Config Context)?

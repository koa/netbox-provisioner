# Namens-Leitfaden

Ein Vorschlag, wie Geräte in NetBox heissen, damit man aus dem Namen sieht, wo ein Gerät steht
und was es tut. Der Provisioner prüft ihn nicht und benennt nichts danach um; er ist eine Hilfe
beim Wählen. Wie ein Name geändert wird, beschreibt [Geräte umbenennen](concepts/device-rename.md).

## Was die Software verlangt

Diese Regeln folgen aus dem Code und gelten unabhängig vom Leitfaden:

- **Eindeutig:** Meldet sich ein Gerät mit dem Namen eines anderen NetBox-Geräts, gilt es als
  falsches Gerät (`WrongDevice`).
- **DNS-tauglich:** Nur Kleinbuchstaben `a`–`z`, Ziffern und `-`, nicht am Anfang oder Ende,
  höchstens 63 Zeichen pro Teil. Sonst steht der Name nicht im WebFig-Zertifikat
  (`www_certificate::wanted`). Umlaute umschreiben (`ue`, `ae`, `oe`). Wie das jq-Skript für
  Blocky andere Namen behandelt, ist nicht geprüft.
- **Kurz:** Der Name steckt in den Port-Namen der Nachbarn (`e02-<name>`, ein Port ohne Label,
  [Namensgenerierung](interface-naming.md)) und in deren Labels („Verbindung“). Lange Namen
  machen lange Interface-Namen.

## Schema

`<ort>-<rolle>[<nr>]`, Richtfunk `<ort>-<gegenort>`.

- **Ort:** ein festes, kurzes Kürzel pro Gebäude oder Hof, für alle Geräte dort gleich. Nach dem
  Ort, nicht nach den Bewohnern: Bewohner wechseln, und wem was gehört, sagt in NetBox der Owner.
- **Rolle:** `router`, `sw` (Switch), `ap` (Access Point), sonst ein Wort für die Aufgabe. Wo es
  hilft, der Raum oder das Stockwerk dahinter: `<ort>-ap-1og`, `<ort>-ap-stall`.
- **Nummer:** nur bei mehreren gleicher Rolle am selben Ort, ohne Bindestrich: `<ort>-sw1`,
  `<ort>-sw2`.
- **Richtfunk:** jedes Ende nach dem eigenen Ort und dem Ort der Gegenseite. So heissen schon
  `berg-schmidrueti` und `schmidrueti-berg`; der Port des Routers davor heisst dann
  `e02-berg-schmidrueti` und sagt, wohin das Kabel führt.
- **Geräte ohne RouterOS** (Drucker, Server, Kamera): `<ort>-<was>`. Der Hostname, den
  „Lease übernehmen“ aus DHCP mitbringt (eine Seriennummer, ein Werksname), ist ein Platzhalter,
  der umbenannt werden sollte.
- **Dosen und Patchpanels:** Sie sind in NetBox Geräte für die Verkabelung, kein Netzgerät; ihr
  Name darf der Raum bleiben („Büro Ecke“). Der Leitfaden gilt für sie nicht.

## Reihenfolge beim Umstellen

Nicht alles auf einmal: zuerst die Ort-Kürzel festlegen, dann pro Site umbenennen und die Site
anwenden (das Gerät und seine Nachbarn ändern Identity, Port-Namen und das Zertifikat). Ein
Tausch zweier Namen sperrt heute die Anmeldung, bis der Helfer aus
[Geräte umbenennen](concepts/device-rename.md) gebaut ist: den Umweg über einen dritten, freien
Namen nehmen.

## Offen

- Die Ort-Kürzel selbst: eins pro Site oder pro Gebäude (eine Site mit mehreren Häusern)?
- Familiennamen in Namen ganz vermeiden oder dort lassen, wo ein Haus so heisst?

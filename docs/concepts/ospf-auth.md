# Konzept: OSPF-Authentisierung pro Segment

Status: Konzept, Aufgabe A10 in [todo.md](../todo.md) (Frage Z44). Vor allem für die Ports
von Anwohnern ([Self-Service](self-service.md#risiken)): Ein Gerät ohne den Schlüssel wird
kein OSPF-Nachbar und kann keine Routen einspeisen.

## Grundsatz

**Credentials werden nur angefasst, wenn positiv feststeht, dass alle Endpunkte des
L2-Segments im selben Batch sind. Im Zweifel: nicht anfassen.** Ohne diese Freigabe übernimmt
der Generator `auth`, `auth-id` und `auth-key` jedes Templates so, wie das Gerät sie hat; es
gibt keinen Weg, auf dem er von sich aus einen Schlüssel setzt, ändert oder entfernt.

## Was RouterOS tut (CHR 7.24.6, gemessen)

- `auth` gilt pro Interface-Template. Liegen zwei Ports im selben Template, bekommen beide
  denselben Schlüssel: Ein Segment für sich braucht ein Template für sich.
- `print` gibt `auth`, `auth-id` und `auth-key` zurück; ein Schlüssel aus 64 Hex-Zeichen geht
  mit `sha256`.
- Unterscheiden sich die Enden im Schlüssel, in `auth-id` oder in `auth`, fällt die
  Nachbarschaft nach dem Dead-Interval (38–40 s) weg, auch eine bestehende; v2 und v3 gleich.
- Wechseln beide Enden zugleich oder 20 s versetzt, bleibt sie `Full`, ohne verlorenen Ping
  (drei Router in einer Kette, Segment für Segment, auch mit verschiedenen Schlüsseln auf einem
  Gerät).
- Ein Port, der aus dem gemeinsamen Template in ein eigenes wandert (zuerst anlegen, dann aus
  dem gemeinsamen nehmen), bleibt `Full`.
- Wechselt nur ein Ende, fällt die Nachbarschaft nach 40 s; sind beide wieder gleich, ist sie
  nach 9 s `Full`, Verkehr geht nach weiteren 7 s wieder (die Route kommt nach `Full`).

## Schlüssel

- Einstellung `ospf_auth` (Helm-Secret wie die Passwörter): Geheimnisse nach Version
  (`secrets: {1: …, 2: …}`, Version 1–255) und die Zielversion `current`. Ohne die Einstellung
  wird nichts angefasst.
- Schlüssel eines Segments: `hex(HMAC-SHA256(secret[v], "<IPv4-Netz des Segments>"))`, 64
  Zeichen, `auth=sha256`, `auth-id=v`. Beide Enden rechnen ihn aus denselben Daten; er steht
  nirgends ausser auf den Geräten.
- Wechsel des Geheimnisses: neue Version eintragen und `current` auf sie setzen. Jedes Segment
  wechselt, sobald ein Batch es freigibt; bis dahin behält es die alte Version (die
  `auth-id` auf dem Gerät sagt welche). Ein Geheimnis kann gehen, wenn kein Segment es mehr
  nutzt.

## Segment

Ein Segment ist ein IPv4-Netz mit `ospf`-Interfaces, nicht ein Kabel: ein Funkbrücken-Segment
hat vier Enden (zwei Router, zwei Brückengeräte), ein Switch ohne RouterOS dazwischen ändert
nichts. So bestimmt auch `ManagementTree` die Nachbarn. Ein Port mit mehr als einem oder
keinem IPv4-Netz ist kein Segment und bleibt unberührt.

Auf dem Gerät hat ein Port mit Auth ein eigenes Template (v2 und v3, Kommentar
`netbox-provisioner: ospf <Netz>`); die übrigen Ports bleiben im gemeinsamen Template ohne
Kommentar.

## Freigabe im Batch

Das Planen eines Batches bekommt einen Schritt vor dem Generieren: Jedes Mitglied liest seine
OSPF-Templates und -Nachbarn, dann entscheidet eine Funktion über alle Mitglieder zusammen,
welche Segmente wechseln dürfen. Ein Segment wird freigegeben, wenn **jede** Bedingung gilt:

1. **NetBox ist vollständig:** Jede Adresse im Netz gehört zu einem `ospf`-Interface eines
   RouterOS-Geräts mit primärer IPv4 (der Router-ID).
2. **Alle Enden sind im Batch:** jedes dieser Geräte ist Mitglied und gelesen.
3. **Live stimmt mit NetBox:** Auf jedem Mitglied sind die Nachbarn am Port, in v2 und v3,
   genau die anderen Enden (nach Router-ID), jeder `Full` oder `2-Way`. Ein fremder oder ein
   fehlender Nachbar ist Zweifel: ein Segment, das unten ist, wird nicht angefasst.
4. **Alle Enden haben heute dasselbe:** gleiche `auth`, `auth-id` und `auth-key` in v2 und v3.

Freigegeben wird das Segment auf alle Enden auf die Zielversion gesetzt. Ein Anwenden allein,
die Geräteseite und die Site-Prüfung geben nie etwas frei. Die Prüfungen nach dem Skript
rechnen mit derselben Freigabe.

## Nach dem Skript

Für jedes gewechselte Segment müssen die Nachbarschaften innerhalb von 3 min wieder stehen
(`Full` oder `2-Way`, wie nach einem Update). Sonst ist der Batch unvollständig und alle
Fallbacks laden gemeinsam die Konfiguration von vorher, mit den alten Schlüsseln.

Die Skripte starten gemeinsam, die Template-Änderung steht aber nicht in jedem Skript an
derselben Stelle. Liegen die Enden mehr als rund 40 s auseinander, fällt die Nachbarschaft
kurz weg und kommt mit dem zweiten Ende wieder; das prüft der CHR-Test mit `applyBatch`.

## Was der Operator sieht

Ein Segment, dessen Stand nicht die Zielversion ist und das nicht wechselt, ist ein Hinweis
beim Anwenden (`ApplyWarning`), mit dem Grund: nicht alle Enden im Batch (welche fehlen), ein
fremder Nachbar (Router-ID), ein erwarteter Nachbar fehlt, die Enden haben verschiedene
Credentials, eine Adresse an einem Gerät ohne RouterOS oder ohne `ospf`.

## Offen danach

- Die Automatik (C20) nimmt die anderen Enden eines ausstehenden Segments in ihre Runde.
- „Gemeinsam anwenden“ wählt die fehlenden Enden vor.

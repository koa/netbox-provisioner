Offene Punkte
============

Status: **offen**, **in Arbeit** oder **erledigt** (mit Hinweis, wie es umgesetzt ist). Erledigte
Punkte bleiben stehen.

Server-Log
==========

Struktur
--------
**Status: erledigt.** Update, Apply und Ersteinrichtung melden Ereignisse (`job::Event`, eine
GraphQL-Union: was heruntergeladen, kopiert, angewendet wird, worauf gewartet wird, was vom
Fallback wurde, die Ausgabe der Konsole), das Frontend formuliert sie auf Deutsch.

Aktuell ist das Server-Log nur ein String. Es sollte als Stream von Events formuliert werden. Ein Event ist z.B. ein Text der von der API zurückgeliefert wird. Ein anderer Event ist, dass der Server auf etwas wartet.
So kann der eigentliche Text für den Benutzer im Frontend gerendert werden, wo auch Übersetzungen möglich sind. Andererseits bleibt der semantische Inhalt erhalten und er kann im UI genutzt werden (z.B. ein Button für eine weitere Nutzeraktion statt nur einem Text).

Handling
--------
**Status: erledigt.** Update, Apply und Ersteinrichtung laufen als Job im Hintergrund: die Seite
zeigt die Ereignisse, sobald sie da sind (sie fragt jede Sekunde nach), auch nach dem Neuladen,
und hat einen Knopf zum Abbrechen. Abgebrochen wird dort, wo es sicher ist (zwischen den
Schritten und beim Warten): ein Update entfernt dann hochgeladene Pakete wieder, ein Apply
lässt nach dem Skript den Fallback stehen (das Gerät lädt die Konfiguration von vorher), die
Ersteinrichtung bricht nicht mitten in der Konsolensitzung ab.

Wenn eine Aktion länger dauert, sollen die Logs nicht im Server gesammelt werden bis alles vorbei ist. Das UI soll live darstellen was passiert, immer mit der Möglichkeit die Aktion abzubrechen, wenn etwas schief läuft.



Werte
=====

Konvertierungen
---------------
**Status: erledigt.** `parse_uptime` ist weg, die Uptime liest `RosValue for Duration`
(mikrotik-model), das dafür Sekunden ohne Einheit (`90`, `1m30`) gelernt hat.

Der Trait RosValue kann Strings von der API parsen (und codieren). Nutze das statt mit parse_uptime einen neuen Parser zu schreiben (erweitere ggf. RosValue, wenn da noch Features fehlen)

Handling
--------
**Status: erledigt** für `device::update` und `device::bootstrap`: MACs sind `MacAddress`
(GraphQL-Scalar `ScalarMac`, eine ungültige MAC lehnt schon die API ab), Versionen, Pakete,
Dateien und Ergebnisse `Box<str>`/`Box<[_]>`. Die gesammelten Logs bleiben `String`, bis sie
Events werden (Server-Log).

Daten werden in einem möglichst nativen Format gespeichert. Z.B. MacAddr für Mac Adressen (Nicht String)
Wo immer ein Wert Immutable ist, eine Immutable Darstellung verfügbar ist und es keine weiteren Nachteile gibt, wird Immutable genutzt (z.B. Box<str> statt String, Box<[_]> statt Vec, etc)


Fallback
========
**Status: erledigt**, statt mit einem berechneten Inversen mit einem Backup: vor dem Anwenden
speichert das Gerät ein Backup und plant, es nach `apply_fallback_seconds` (180) zu laden
(`/system/scheduler`). Erreicht der Provisioner das Gerät danach wieder, nimmt er beides weg,
sonst startet es mit der Konfiguration von vorher neu. Das Backup bringt alles zurück, auch was
das Modell nicht kennt, und braucht kein Inverses (hinzugefügte Einträge haben vorher keine
`.id`, die Reihenfolge der Firewall müsste wiederhergestellt werden); der Preis ist der
Neustart, nur wenn der Fallback auslöst. Gemeint ist das Anwenden einer Konfigurationsänderung.

Es braucht noch einen Fallback Mechanismus für Updates.
D.h.: Befor ein Update angewendet wird, wird das Inverse Update berechnet (D.h. wie kann man dieses Update rückgängig machen). Dieses Update muss zu Begin des Updates auf das Gerät geschrieben und für die Automatische Ausführung geplant werden, für den Fall dass das Update schief geht.
Wenn das Update fertig ist und das Gerät weiterhin erreichbar ist, wird dieser Fallback gestoppt.

Version-Upgrade
===============

Default: Long-Term
------------------
**Status: erledigt.** Ohne `routeros_channel` ist `long-term` vorgewählt.

Konzept für Updates im ganzen Netz
----------------------------------
**Status: offen** (baut auf dem Server-Log auf).

Konzept für Updates im ganzen Netz fehlt


Visualisierung Netz-Zustand
===========================

Es braucht eine einfache Übersicht über alle Verbindungen.

Daten
-----
Baue einen Graph wahlweise basierend auf den OSPF2 oder OSPF3 LSA um den Ist-Zustand aller Strecken zu sehen (Soll-Zustand kommt von Netbox). So kann man vielleicht den aktuellen Netz-Zustand mit hoher Frequenz (ca. alle 5s) von nur einem OSPF-Router pollen. Alternative Idee: Einen Aggregator der Log-Meldungen zu OSPF Link Zuständen von allen Routern empfängt und damit das Netz live aktualisiert.

Visualisierung
--------------
SVG Darstellung von einem Netz. Kanten sind Verbindungen, Knoten sind Router/Endpunkte. Ähnlich die die Darstellung von Neo4J.
Knoten haben Icon nach Typ des Routers/Endpunkt, Farbe nach Zustand (Grün: alles ok, Rot: unerreichbar, Orange: Erreichbar mit Problemen). Text: Name des Knoten.
Kanten haben Farben nach Zustand (Grün: alles ok, Rot: tot, Orange: dazwischen).
Interaktiv: Benutzer kann Knoten verschieben, Knotenpositionen werden gespeichert
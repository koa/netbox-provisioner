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

Für Updates: **Status: in Arbeit**, siehe [Update-Fallback](concepts/update-fallback.md).
Stufe 1 ist umgesetzt: Vor dem Update kopiert der Provisioner auf die zweite Partition, ein
Scheduler schaltet ohne Erfolg dorthin zurück, und die Geräteseite bietet „Von partN starten“.
Von Stufe 2 erreicht er ein Gerät schon über den Nachbarn; offen sind die Rettung über
mac-telnet und der Downgrade auf Knopfdruck.

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
**Status: offen.** Konzept: [Updates im ganzen Netz](concepts/network-update.md), nichts
umgesetzt. Was es braucht, ist schon da: Server-Log, Update-Fallback, PoE-Leases und das
Aussetzen der Watchdogs.

Konzept für Updates im ganzen Netz fehlt


Visualisierung Netz-Zustand
===========================

**Status: offen.** Konzept: [Netz-Karte](concepts/network-map.md), nichts umgesetzt.

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
Information über Health zugänglich (Spannung, Strom, Temperatur, ...)
Visualisierung der PoE Versorgungen (Check Netbox-Doku, Anzeige Stromfluss)



Device-Darstellung
==================
**Status: in Arbeit**, siehe [Geräteseite](concepts/device-page.md). Karten im Raster, auf dem
Handy einspaltig; „Ziel ändern“ ist jetzt „Zugang“ im Kopf und in der Meldung „nicht
erreichbar“. Offen: ein Bereich für Liegengebliebenes auf dem Gerät und Befunde.

Die Darstellung ist recht chaotisch. Das sollte besser angeordnet werden. Wenn möglich auch für Mobile passend.
Gibt es noch einen validen Anwendungsfall für "Ziel ändern". Wenn nicht, weg damit.


Polling verbessern
==================
**Status: in Arbeit**, siehe [Jobs](concepts/job-polling.md). Ein Fehler beim Abfragen hält den
Job nicht mehr an: Das Frontend fragt mit wachsendem Abstand weiter und beendet die Ansicht bei
einem unbekannten Job. Die Job-Nummern sind über Neustarts eindeutig. Offen: Liegengebliebenes
auf dem Gerät anzeigen, Events speichern, geordnetes Beenden.
 Ein einzelner Fehler beim Polling führt dazu, dass der Client den ganzen Vorgang abbricht. Das sollte so nicht sein (Der Fehler soll angezeigt werden und der Nutzer soll die Möglichkeit haben den Vorgang abzubrechen, aber es muss auch weiter gehen können)

 Config-Helper
==============
**Status: in Arbeit**, siehe [Config-Helper](concepts/config-helper.md). Umgesetzt sind die
Befunde, „Nachbarn lesen“ und die Helfer Loopback, Verbindung (samt „Kabel übernehmen“ und
„Verbindung abräumen“), Flags, Kunden-Netz (IPv4 und IPv6), PoE und Funkbrücke. Offen ist vor
allem „Neues Gerät“.

Die Konfiguration über Netbox ist sehr flexibel, aber auch sehr aufwendig und fehlerträchtig. Für Standart-Situationen braucht es ein einfaches UI im Netbox Provisioner.
- Hinzufügen von einem neuen Gerät (Standort, Name, Typ erkennen oder eingeben)
- Primäre IP-Adresse für Loopback aus Prefix zuweisen
- IP-Adressen für Direkte Verbindunge, WLAN-Bridge-Verbindungen, lokale Clients aus entsprechenden Prefixes laden und zuweisen
- OSPF und DHCP-Flags setzen
- PoE Beziehungen checken/ändern in Netbox
- WLAN Bridge konfigurieren (Gerätetyp auswählen, beide Geräte automatisch passend in Netbox konfigurieren)

Graphql Files
=============
**Status: offen.** Konzept: [GraphQL-Dateien](concepts/graphql-files.md). Heute sind die Abfragen
nach Seite gruppiert, z.B. 11 in `netbox-helper.graphql`.
Erstelle für jeden Graphql-Query ein eigenes .graphql File


DSL für Mikrotik Kommandos
==========================
**Status: offen.** Konzept: [Befehle ohne Strings](concepts/command-dsl.md). Die Konfiguration
ist schon getypt (mikrotik-model). Strings sind noch die einzelnen API-Befehle (`device::raw`) und
die Skripte der Scheduler und der Ersteinrichtung.

Alle Mikrotik-Befehle sind aktuell strings. Prüfe die Möglichkeit dafür eine DSL mit Makros oder ähnlich zu bauen um die Semantik besser beizubehalten.

Provisionierung
===============
**Status: offen.** Konzept: [Deployment](concepts/deployment.md).
Für das Rollout braucht die Applikation:
- Dockerfile basierend auf bare (kein volles Linux)
- Helm-Chart
- Github Actions für Build und rollout (snapshot für jeden commit, abgeleitet vom Hash, alte Snapshots automatisch löschen, Echte, dauerhafte Release-Version für prod)


User Journeys
=============

Hier ein paar User Journeys, die gut funktionieren sollen. Bitte nicht das UI als Wizzard entlang dieser Journeys ausrichten, sondern nur die Aufteilung der Funktionen so anpassen, dass diese Journeys gut unterstützt sind.
Diese Journeys sollen zuerst nur die Struktur der Gesamtlösung führen. Sie sollen nicht alle sofort implementiert werden.

Rollout
-------
**Status: in Arbeit**, siehe [User Journeys](concepts/user-journeys.md#rollout). Ersteinrichtung,
Helfer und gemeinsames Anwenden gibt es; es fehlt vor allem „Neues Gerät“.
Neue Geräte werden installiert. Diese brauchen neue Verbindungen (Funk oder Direkt), Loopback-Adressen und ggf Kunden-Netze.
Gerade neue Verbindungen sind sehr mühsam direkt in Netbox einzurichten (Geräte, IP-Adressen, Prefixe, etc. alles über mehrere Masken verteilt) und haben ein grosses Risiko für Fehler.
Idee: Man verbindet die neuen Geräte im Feld und konfiguriert diese Fortlaufend mit dem Mobiltelefon. Die Geräte kommen laufend online und man kann sie gleich testen (ohne zu Hause am PC das Netz in Netbox nachzeichen zu müssen)

Datenkorrektur
--------------
**Status: in Arbeit**, siehe [User Journeys](concepts/user-journeys.md#datenkorrektur). Befunde
und „Nachbarn lesen“ gibt es; es fehlen eine Übersicht pro Site, wie weit jedes Gerät von NetBox
abweicht, und das Übernehmen von Adressen, die nur auf dem Gerät stehen.
Die Daten in Netbox und der Realität sind voneinander abgewichen. Die Abweichungen sollen möglichst einfach erkannt und korrigiert werden können. Initial müssen mal alle Geräte "synct" werden, dass man sie später ohne grössere Risiken ausrollen kann.

Überwachung
-----------
**Status: offen.** Konzept: [Überwachung](concepts/monitoring.md).
Jeder Anwender soll auf den ersten Blick sehen können wie gut das Netz läuft. Einerseits für seine eigenen Bedürfnisse (Wie gut geht es der Internetverbindung?) und auch für einen Gesamtüberblick (Was muss kontrolliert/gewartet werden?).

Firewall
--------
**Status: offen.** Konzept: [Firewall](concepts/firewall.md). Heute erzeugt der Generator nur
Masquerading und das Schliessen von DNS an `nat`-Uplinks.
Das Netz hat einige wenige Firewalls. Diese sollen in Netbox verwaltet werden.
- Port-Forwarding IPv4
- Address-Forwarding IPv6
- https://manual.mikrotik.com/docs/firewall-and-quality-of-service/user-guides/bruteforce-prevention (IPv4 und IPv6)

Self-Service
------------
**Status: offen.** Konzept: [Self-Service](concepts/self-service.md), zusammen mit den Personas.
Anwender sollen ein paar einfache Handgriffe selbst ausführen können
- Anpassung SSID / Passwort
- Physische Stecker (Switch-Ports) einem WLAN-VLAN zuordnen
- Liste der IP Leases in den eigenen Netzen


Personas
========
**Status: offen.** Teil des Konzepts [Self-Service](concepts/self-service.md): Heute dürfen alle,
die sich anmelden dürfen, alles.

Admin
-----
Ein Admin richtet das Netz ein und löst die Probleme im ganzen Netz. Er kann alles sehen und anpassen.

Anwohner (Usergruppen)
----------------------
Ein Anwohner besitzt VLANs mit ihren WLANs und eine Liste von Switch-Ports oder ganze Geräte.
Er kann:
- SSID / Passwort ändern
- Ports in seinem Besitz umkonfigurieren (VLAN oder AP anschliessen)
- Zusätzliche APs anschliessen und in seine WLAN-Gruppe aufnehmen
- Seine IP Leases sehen

Konfiguration mit 2 Partitionen
===============================

Wenn ein Gerät 2 Partitionen hat, muss nach einer erfolgreichen Konfigurationsänderung die 2. Partition auch nachgezogen werden (Bei einer instabilen Stromversorgung schaltet Mikrotik manchmal automatisch auf die 2. Partition weil es fälschlicherweise ein Boot-Problem erkannt hat) 
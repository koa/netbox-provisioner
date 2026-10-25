Offene Punkte
============

Ein Punkt verschwindet von hier, sobald er erledigt ist oder als Aufgabe in [todo.md](todo.md)
steht (erledigte Aufgaben in [done.md](done.md)).

Konfiguration mit 2 Partitionen
===============================

Wenn ein Gerät 2 Partitionen hat, muss nach einer erfolgreichen Konfigurationsänderung die 2. Partition auch nachgezogen werden (Bei einer instabilen Stromversorgung schaltet Mikrotik manchmal automatisch auf die 2. Partition weil es fälschlicherweise ein Boot-Problem erkannt hat)

Port der Angeschlossenen Nicht-Mikrotik Geräte erkennen
=======================================================

Wenn ein Gerät eine Dokumentierte IPV4- oder IPv6-Adresse hat, ist es möglich über ARP, Neighborhood und Bridge forwarding tabellen festzustellen an welchem Port diese angeschlossen sind. Daraus sollen fehlende Kabelverbindungen erkannt und in Netbox nachgetragen werden.


## Subscription
Polling vom Client produziert viel unnötigen Datenverkehr. Implementiere Subscriptions stattdessen

## Readme
Schreibe ein passendes Readme ins root-Verzeichnis. Beschreibe darin grob um was es bei dem Projekt geht, wie man es aufsetzt und verweise für die Details auf die entsprechenden Docs
Server-Log
==========

Struktur
--------
Aktuell ist das Server-Log nur ein String. Es sollte als Stream von Events formuliert werden. Ein Event ist z.B. ein Text der von der API zurückgeliefert wird. Ein anderer Event ist, dass der Server auf etwas wartet.
So kann der eigentliche Text für den Benutzer im Frontend gerendert werden, wo auch Übersetzungen möglich sind. Andererseits bleibt der semantische Inhalt erhalten und er kann im UI genutzt werden (z.B. ein Button für eine weitere Nutzeraktion statt nur einem Text).

Handling
--------
Wenn eine Aktion länger dauert, sollen die Logs nicht im Server gesammelt werden bis alles vorbei ist. Das UI soll live darstellen was passiert, immer mit der Möglichkeit die Aktion abzubrechen, wenn etwas schief läuft.



Werte
=====

Konvertierungen
---------------

Der Trait RosValue kann Strings von der API parsen (und codieren). Nutze das statt mit parse_uptime einen neuen Parser zu schreiben (erweitere ggf. RosValue, wenn da noch Features fehlen)

Handling
--------

Daten werden in einem möglichst nativen Format gespeichert. Z.B. MacAddr für Mac Adressen (Nicht String)
Wo immer ein Wert Immutable ist, eine Immutable Darstellung verfügbar ist und es keine weiteren Nachteile gibt, wird Immutable genutzt (z.B. Box<str> statt String, Box<[_]> statt Vec, etc)


Fallback
========

Es braucht noch einen Fallback Mechanismus für Updates.
D.h.: Befor ein Update angewendet wird, wird das Inverse Update berechnet (D.h. wie kann man dieses Update rückgängig machen). Dieses Update muss zu Begin des Updates auf das Gerät geschrieben und für die Automatische Ausführung geplant werden, für den Fall dass das Update schief geht.
Wenn das Update fertig ist und das Gerät weiterhin erreichbar ist, wird dieser Fallback gestoppt.

Version-Upgrade
===============

Default: Long-Term
Konzept für Updates im ganzen Netz fehlt
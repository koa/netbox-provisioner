# Konzept: Automatisches Rollout

Status: Konzept, nicht umgesetzt. Aufgabe C20 in [todo.md](../todo.md). Voraussetzung für den
[Self-Service](self-service.md) ab dem Passwort der WLANs: Was ein Anwohner in NetBox ändert,
soll die Geräte erreichen, ohne dass jemand von Hand anwendet.

## Ziel

Teile des Netzes sind **automatisch**: Eine Änderung in NetBox wird erkannt, ihre Auswirkungen
werden für jedes automatische Gerät berechnet und gemeinsam als **eine Transaktion**
ausgerollt. Scheitert ein Gerät, gehen alle zurück, und die Automatik des Bereichs steht in
einem **Fehlerzustand**. Der Admin hebt ihn auf oder wendet die Geräte einzeln an, wie heute.

Die Transaktion ist ein automatisch ausgelöstes [Gemeinsam anwenden](../jobs.md#gemeinsam-anwenden)
(`applyBatch`): ein Fallback für alle, die Skripte starten gleichzeitig, und die Fallbacks gehen
nur weg, wenn jedes Gerät durch ist; sonst laden alle ihre Konfiguration von vorher. Neu sind
das Auslösen, der Bereich, die Prüfung des Internets und der Fehlerzustand.

## Bereich

- **Custom Field `auto_rollout` an der Site** (Boolean, nur Admins): Alle Geräte der Site sind
  automatisch.
- **Ausnahme pro Gerät:** ein Custom Field am Gerät, das es manuell lässt (z.B. ein heikler
  Core-Router in einer automatischen Site). Name noch offen.
- **Einschalten** geht nur, wenn alle Geräte der Site einen leeren Diff haben. Sonst zeigt der
  Provisioner die Diffs; der Admin wendet sie zuerst an. Sonst ginge mit der ersten Änderung
  alles mit, was sich vorher angesammelt hat.

## Auslösen

- **Erkennen:** eine Event Rule in NetBox, die einen Webhook an den Provisioner schickt, oder
  das Abfragen des Changelogs (offen). Schreibt der Provisioner selbst (Anwohner, Helfer), stösst
  er das Rollout direkt an.
- **Bündeln:** Ein Admin ändert mehrere Objekte nacheinander, ein Plan schreibt viele. Gerechnet
  wird erst nach einer Ruhezeit ohne weitere Änderung (Länge offen); was in der Zeit dazukommt,
  gehört zur selben Transaktion.
- **Warteschlange:** Läuft schon eine Transaktion, warten neue Änderungen und gehen danach
  gemeinsam als nächste hinaus.
- **Ausschluss:** Manuelle Aufträge auf einem automatischen Gerät (Anwenden, Update, Neustart)
  und eine Transaktion schliessen sich aus; wer zuerst kommt, läuft, der andere wartet.

## Auswirkungen

Nach einer Änderung rechnet der Provisioner den Diff jedes automatischen Geräts neu (der
Generator ist deklarativ: der Diff ist alles, was vom Stand in NetBox abweicht). Geräte mit
leerem Diff sind nicht betroffen. Dann:

- **Ein manuelles Gerät betroffen** (der WLAN-Controller in einer anderen Site, der OSPF-Nachbar
  am anderen Ende einer Strecke, ein VTEP desselben VXLAN): Die Automatik rollt **nicht
  teilweise** aus. Die Änderung wartet, bis ein Admin sie freigibt (dann mit dem manuellen Gerät
  in der Transaktion) oder die Geräte von Hand anwendet.
- **Von Hand geändert:** Was auf einem automatischen Gerät NetBox widerspricht, geht mit der
  nächsten Transaktion zurück: der Diff ist immer der ganze Unterschied zu NetBox, nicht nur der
  der Änderung. Welcher Teil von Hand kam, lässt sich nicht verlässlich erkennen (ein früher
  gescheitertes Anwenden sieht gleich aus), darum wird es nicht versucht. Damit nachzulesen ist,
  was überschrieben wurde, nennt der [Journal-Eintrag](../protocol.md) eines automatischen
  Anwendens die Änderungen pro Tabelle (heute nennt er nur, ob angewendet wurde). Ein
  automatisches Gerät ändert man darum in NetBox, nicht auf dem Gerät.
- **Nicht erreichbar vor dem Start:** Ein Gerät, das schon vor der Transaktion nicht erreichbar
  ist (ein Anwohner hat seinem AP den Strom gezogen), wird **zurückgestellt**: Die Transaktion
  läuft ohne es, es bekommt die Änderung nachgeholt, sobald es wieder da ist (im Anwohner-UI
  „wartet“). Sonst hielte ein einziges ausgeschaltetes Gerät die Automatik eines ganzen Bereichs
  an. Ist ein Ende einer Funkstrecke nicht erreichbar, wird auch das andere zurückgestellt: eine
  Änderung an nur einer Seite kann die Strecke trennen.
- **Erreichbar, aber ohne Internet:** Erreicht ein Gerät das Internet nicht, beginnt die
  Transaktion gar nicht (siehe [Internet](#internet)); es wird nicht zurückgestellt.
- **Nie automatisch** (die Änderung wartet auf einen Admin):
  - was den Weg des Provisioners zu einem Gerät ändert (Management-Adresse, MTU auf dem Weg);
  - was nach aussen etwas ankündigt, das nicht in den freigegebenen Bereichen liegt (OSPF am
    Uplink; das Lab hat einmal eine Default-Route in die Produktion exportiert);
  - ein anderer Anschluss eines Geräts, der Umstecken verlangt: Der Fallback nähme die Änderung
    zurück, bevor jemand umgesteckt hat. Übernimmt der Anwohner den erkannten Anschluss (das
    Kabel steckt schon dort), ist es kein Umstecken.

  Funkänderungen sind erlaubt, verlängern den Fallback aber um 24 min (DFS).

## Internet

Die Automatik darf das Netz nicht vom Internet trennen. Geprüft wird **von jedem Gerät der
Transaktion aus** (`/ping` auf dem Gerät, über seine eigene Routing-Tabelle), nicht aus jedem
Netz der Anwohner: Es geht darum, dass das Netz im Grossen und Ganzen geht und das Management
noch funktioniert. **IPv6 zählt gleich wie IPv4.**

- **Vorher:** Eine Transaktion beginnt nur, wenn jedes ihrer erreichbaren Geräte das Internet
  über IPv4 **und** IPv6 erreicht; ein einziges ohne genügt, dass gar nichts automatisch geschieht. Sonst beginnt sie nicht; die Änderung wartet in der Warteschlange. Das
  ist kein Fehlerzustand: ein Ausfall beim Provider soll die Automatik nicht stilllegen, sie geht
  weiter, wenn das Internet zurück ist.
- **Nachher:** Die Fallbacks gehen nur weg, wenn jedes Gerät das Internet über IPv4 und IPv6
  danach noch erreicht. Eine Bedingung mehr am Übergang `SkriptLäuft → FallbackWegnehmen` des
  gemeinsamen Anwendens: `[alle sind durch, erreicht, in Reihe und im Internet]`. Sonst laden
  alle ihre Konfiguration von vorher, und der Bereich geht in den Fehlerzustand.
- **Wiederholt bis zur Frist:** Nach dem Skript braucht OSPF eine Weile, bis die Routen wieder
  stehen. Geprüft wird wiederholt; gescheitert ist es erst, wenn bis kurz vor dem Fallback kein
  Ziel antwortet. Die Frist darf dafür nicht knapp sein.
- **Ziele:** mehrere pro Adressfamilie in der Konfiguration des Provisioners (welche, offen);
  erreichbar heisst, mindestens eines antwortet, damit ein einzelner Dienst kein Internet-Ausfall
  ist.

## Fehlerzustand

- **Pro Bereich (Site)**, nicht global: Ein Fehler in einer Site hält die anderen nicht an.
  Betrifft eine Transaktion mehrere Sites, gehen alle in den Fehlerzustand.
- **NetBox bleibt**, wie es ist; nur die Geräte gehen zurück. Das ist die sichtbare Abweichung,
  die das Anwohner-UI „nicht ausgerollt“ nennt (Z51).
- Weitere Änderungen sammeln sich, solange der Bereich im Fehler ist. Hebt der Admin ihn auf,
  zeigt der Provisioner die Vorschau aller gesammelten Änderungen und rollt sie gemeinsam aus.
  Oder der Admin wendet Geräte einzeln an; sind danach alle Diffs leer, ist der Bereich wieder
  bereit.
- **Melden:** als Metrik an Prometheus, den Alarm macht der Alertmanager (Z13).

## Festhalten

Heute lebt das meiste nur im Speicher des Prozesses ([Jobs](../jobs.md#weiterführen-nach-einem-neustart)).
Die Automatik muss einen Neustart des Servers überleben:

- der Zustand jedes Bereichs (bereit, läuft, wartet aufs Internet, Fehler mit Grund);
- die Warteschlange der Änderungen;
- pro Änderung: wer sie ausgelöst hat (im Changelog von NetBox steht immer der Dienst-Benutzer
  des Provisioners; die Support-Ansicht will „koa (Admin) für Familie Meier“), welche Geräte sie
  betrifft und deren Stand (ausstehend, ausgerollt, wartet, zurückgestellt, gescheitert). Das
  Anwohner-UI zeigt diesen Stand pro Element.

## Offen

- Webhook aus NetBox oder Abfragen des Changelogs, und die Länge der Ruhezeit.
- Die Ziele für den Internet-Test (IPv4 und IPv6).
- Der Name des Custom Fields, das ein Gerät in einer automatischen Site manuell lässt.
- Wo der Zustand gespeichert wird (neben dem, was ein Auftrag heute für das Weiterführen
  speichert, oder eigene Tabellen).
- Wie „nie automatisch“ im Diff erkannt wird (Weg des Provisioners, Ankündigungen).
- Ob Änderungen von Hand auch ohne NetBox-Änderung zurückgehen (z.B. eine Prüfung jede Nacht),
  oder erst mit der nächsten.

## Entschieden (2026-11-17)

- **Bereich:** Custom Field `auto_rollout` an der Site, mit einer Ausnahme pro Gerät.
- **Einschalten** nur mit leeren Diffs; was danach von Hand auf ein automatisches Gerät kommt
  und NetBox widerspricht, nimmt die nächste Transaktion zurück, im Journal nachzulesen.
- **Ein manuelles Gerät betroffen:** kein teilweises Rollout, ein Admin gibt frei.
- **Vorher nicht erreichbare Geräte** werden übergangen und bekommen die Änderung nachgeholt,
  ebenso das andere Ende ihrer Funkstrecken; nur ein Fehler während der Transaktion nimmt alles
  zurück.
- **Internet:** von jedem Gerät geprüft, nicht aus jedem Netz; IPv4 und IPv6 gleich wichtig.
  Erreicht ein erreichbares Gerät nicht beides, geschieht nichts automatisch; danach müssen beide
  noch gehen, sonst geht alles zurück.

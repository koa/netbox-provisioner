# Reiner L2-Switch als Extender

Ein alter Switch ohne L3-Offload oder mit anderen Problemen (CRS2xx, CRS354) hängt an einem
modernen Switch oder Router (dem L3-Switch). In NetBox wird er modelliert wie jedes andere Gerät,
mit seinen L3-Funktionen (Adressen, DHCP, OSPF-Netze). Der Provisioner setzt diese Funktionen aber
nicht auf dem alten Switch um, sondern auf dem L3-Switch: Er erzeugt für jede ein VLAN, führt es
über den Trunk zwischen den beiden und baut die L3-Funktion dort auf. Auf dem alten Switch bleibt
reines L2 und seine eigene Management-Adresse.

## Entschieden

- **Beziehung** (Z39, 2026-11-07): ein Custom Field am Interface des alten Switches (Boolean,
  etwa `l3_uplink`, „L3 über diesen Port“). Gesetzt auf dem Port zum L3-Switch heisst es: Alle
  L3-Funktionen dieses Geräts laufen über diesen Port und werden am Gerät am anderen Ende seines
  Kabels umgesetzt. Der Port ist der Trunk, das Kabel gibt den Partner. Der 802.1Q-Modus
  „Tagged (All)“ passt nicht: Die VLANs entstehen erst aus den L3-Funktionen.
- **VLAN-IDs** (2026-11-07): automatisch, aus einem Bereich, der auf beiden beteiligten Switches
  unbenutzt ist.

## Was funktionieren muss

Alle Varianten, wie eine L3-Funktion am alten Switch hängt:

1. **Port mit IP**: Der Port wird Access-Port eines neuen VLANs, das VLAN geht tagged über den
   Trunk, auf dem L3-Switch liegt die IP auf dem VLAN-Interface.
2. **Port mit VLAN**: Das VLAN des Ports geht über den Trunk; die IP (und DHCP usw.) des
   VLAN-Interfaces liegt auf dem L3-Switch.
3. **Bridge mit IP**: Die Ports der Bridge werden Access-Ports eines neuen VLANs, sonst wie 1.
4. **Bridge mit VLAN**: wie 2, mit den Ports der Bridge.

Bei den VLAN-Varianten (2, 4) kann dasselbe VLAN auf dem L3-Switch auch eigene Ports haben oder
über weitere Switches gehen, muss aber nicht: Dann ist es auf dem L3-Switch ein Netz mit den
Ports beider Switches, und die L3-Funktion gibt es einmal.

## Offen

- **Stabile IDs**: Automatisch gewählte VLAN-IDs sollen bei jedem Apply gleich bleiben, auch wenn
  in NetBox eine L3-Funktion dazukommt. Vorschlag: Was der Trunk schon trägt, behält seine ID;
  Neues bekommt die kleinste freie.
- **Die VLAN-ID bei 2 und 4**: das VLAN mit seiner eigenen ID, wenn sie auf dem L3-Switch frei
  oder dasselbe VLAN ist; sonst eine automatisch gewählte (auf beiden Seiten dieselbe).
- **Befunde**: Kabel am `l3_uplink`-Port endet nicht an einem RouterOS-Gerät; der Port hängt in
  einer Bridge mit anderen Ports; die Gateway-Regel (ein L3-Gateway pro VLAN,
  `VlanWithSeveralGateways`) muss die verschobene Funktion dem L3-Switch zurechnen.
- **Generator**: Der alte Switch bekommt nur noch L2 (VLAN-Filter im Switch-Chip, wo er es kann,
  E17), der L3-Switch die Funktionen dazu, beide im selben Apply oder in einem Batch.

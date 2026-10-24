//! Text tables of the newspaper and the message inbox, as the bytes they are
//! in the DGROUP of `BMMAIN.EXE` (`docs/re/logic/news.md` §8 and Appendix A).
//! The game fonts draw `[`=Ä, `\`=Ö, `]`=Ü, `{`=ä, `|`=ö, `}`=ü and `~`=ß.

/// DS:5558.. the 110 body templates (pointer table DS:90d2), NUL-separated in
/// index order, exactly as in DGROUP. The template engine reads it like the
/// original's far pointer, so scans that cross a template's NUL behave alike.
pub(super) const BODY_IMAGE: &[u8] = concat!(
    // 0 (group 0, DS:5558)
    "EIN %x20LANGWEILIGES#M[SSIGES#GUTES#TOLLES#% SPIEL %a %x10GEGEN#BEI#% %b ENDETE %0:%1.\0",
    // 1 (group 0, DS:55af)
    "MIT %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% ENDETE DAS %x20M]DE#M[SSIGE#RECHT ANSPRECHENDE#BEGEISTERNDE#% SPIEL %a %x10GEGEN#IN#% %b.\0",
    // 2 (group 0, DS:5643)
    "EIN %0:%1 SAHEN DIE ZUSCHAUER IM SPIEL %a %x10GEGEN#IN#% %b.\0",
    // 3 (group 0, DS:5680)
    "%0:%1 ENDETE DIE %x20EINSCHL[FERNDE#SCHWACHE#ORDENTLICHE#SEHR GUTE#% PARTIE %a %x10GEGEN#IN#% %b.\0",
    // 4 (group 0, DS:56e2)
    "ZU %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% KAM %a IM SPIEL %x10GEGEN#IN#% %b.\0",
    // 5 (group 0, DS:573e)
    "DIE %x20SEHR SCHWACHE#MITTELM[SSIGE#RECHT ANSPRECHENDE#GUTE#% PARTIE %a %x10GEGEN#IN#% %b ENDETE MIT %x30EINEM UNENTSCHIEDEN#EINEM SIEG#EINER NIEDERLAGE#% F]R %t'S MANNSCHAFT.\0",
    // 6 (group 0, DS:57ee)
    "IN EINER %x20EINSCHL[FERNDEN#DURCHSCHNITTLICHEN#DURCHWACHSENEN#MITREISSENDEN#% PARTIE %x30SPIELTE#SIEGTE#UNTERLAG#% %a %x10GEGEN#BEI#% %b MIT %0:%1.\0",
    // 7 (group 0, DS:5883)
    "DIE %x20LANGWEILIGE#NICHT SEHR ANSEHNLICHE#RECHT GUTE#WIRKLICH ANSPRECHENDE#% PARTIE ZWISCHEN %a %x10GEGEN#BEI#% %b ENDETE %0:%1 %x30UNENTSCHIEDEN#F]R %a#GEGEN %a#%.\0",
    // 8 (group 1, DS:5929)
    "%9 ZUSCHAUER %x30SAHEN#FEIERTEN#BETRAUERTEN#% IHRE MANNSCHAFT.\0",
    // 9 (group 1, DS:5968)
    "%9 ZUSCHAUER VERLIESSEN DAS STADION %x30MIT GEMISCHTEN GEF]HLEN#BEGEISTERT VON IHREM TEAM#ENTT[USCHT VON IHRER MANNSCHAFT#%.\0",
    // 10 (group 1, DS:59e5)
    "DIE %9 ZUSCHAUER ,SOWEIT SIE FANS VON %a WAREN, %x30SAHEN WENIG ANGETAN#WAREN BEEINDRUCKT#MUSSTEN ENTT[USCHT ERLEBEN#%, WIE IHRE MANNSCHAFT SPIELTE.\0",
    // 11 (group 1, DS:5a7a)
    "DIE OFFIZIELLE ZUSCHAUERZAHL LAG NACH ANGABEN DER STADIONLEITUNG BEI %9.\0",
    // 12 (group 1, DS:5ac3)
    "DIE %9 FANS IM STADION %x30SAHEN EINE %x40SCHWACHE#RECHT GUTE#% VORSTELLUNG#FEIERTEN DEN %x50VERDIENTEN#ETWAS GL]CKLICHEN#% ERFOLG#BETRAUERTEN DIE %x50LEISTUNGSGERECHTE#UNGL]CKLICHE#% NIEDERLAGE#% IHRER MANNSCHAFT.\0",
    // 13 (group 2, DS:5b9a)
    "DIE MITGEREISTEN SCHLACHTENBUMMLER WAREN %x30UNZUFRIEDEN#BEGEISTERT#ENTT[USCHT#%.\0",
    // 14 (group 2, DS:5bec)
    "DIE MITGEREISTEN FANS %x30VERHIELTEN SICH RUHIG#FEIERTEN NOCH LANGE NACH DEM SCHLUSSPFIFF#VERLIESSEN DAS STADION SEHR SCHNELL#%.\0",
    // 15 (group 2, DS:5c6d)
    "DIE ANGEREISTEN FANS %x30WUSSTEN NICHT, OB SIE MIT DEM UNENTSCHIEDEN ZUFRIEDEN SEIN SOLLTEN ODER NICHT#ERLEBTEN EINE ANSPRECHENDE LEISTUNG IHRER MANNSCHAFT, UND FEIERTEN DEN SIEG ]BERSCHW[NGLICH#WAREN ENTT[USCHT ]BER DEN AUSGANG DER PARTIE#%.\0",
    // 16 (group 2, DS:5d60)
    "DIE MITGEREISTEN FANS IM STADION %x30SAHEN EINE %x40SCHWACHE#RECHT GUTE#% VORSTELLUNG#FEIERTEN DEN %x50DURCHAUS VERDIENTEN#ETWAS GL]CKLICHEN#% ERFOLG#BETRAUERTEN DIE %x50LEISTUNGSGERECHTE#UNGL]CKLICHE#% NIEDERLAGE#% IHRER MANNSCHAFT BEI %b.\0",
    // 17 (group 3, DS:5e51)
    "DIE HARTE SPIELWEISE DER MANNSCHAFTEN LIESS KEIN TECHNISCH SCH|NES SPIEL ZU.\0",
    // 18 (group 3, DS:5e9e)
    "BEIDE MANNSCHAFTEN GINGEN SEHR HART, TEILWEISE UNFAIR ZUGANGE.\0",
    // 19 (group 3, DS:5edd)
    "DAS ]BERAUS K|RPERBETONTE, HARTE SPIEL DER TEAMS LIESS FL]SSIGE KOMBINATIONEN NICHT ZU.\0",
    // 20 (group 3, DS:5f35)
    "BEIDE TEAMS GINGEN HOCHMOTIVIERT IN DIESES SPIEL. BESONDERS AUFFALLEND WAR DAS ]BERHARTE SPIEL DER MANNSCHAFT %a.\0",
    // 21 (group 3, DS:5fa7)
    "DAS SPIEL WAR GEPR[GT VON VIELEN KLEINEN FOULS, DER UNPARTEISCHE HATTE TEILWEISE ALLE M]HE, DIE KONTROLLE ZU BEHALTEN.\0",
    // 22 (group 3, DS:601e)
    "F]R DEN UNPARTEIISCHEN WAR DAS SPIEL EIN HARTES ST]CK ARBEIT, DA BEIDE TEAMS OFT ]BER DIE GRENZEN DES ERLAUBTEN HINAUS GINGEN.\0",
    // 23 (group 3, DS:609d)
    "DER ]BERHARTE EINSATZ BEIDER MANNSCHAFTEN MU~TE VOM UNPARTEIISCHEN ALLZU OFT UNTERBROCHEN WERDEN.\0",
    // 24 (group 3, DS:60ff)
    "DIE SPIELER %x03GINGEN MIT ENTSCHIEDEN ZUVIEL EINSATZ ZUR SACHE#BESIEGTEN %b MIT SEHR HARTEM EINSATZ#KONNTEN DIE NIEDERLAGE AUCH NICHT DURCH DEN TEILWEISE ERSCHRECKEND HARTEN EINSATZ ABWENDEN#%.\0",
    // 25 (group 4, DS:61c2)
    "DAS GESAMTE SPIEL VERLIEF [USSERST FAIR.\0",
    // 26 (group 4, DS:61eb)
    "BEIDE MANNSCHAFTEN BEM]HTEN SICH UM EIN FAIRES SPIEL.\0",
    // 27 (group 4, DS:6221)
    "%a FIEL DURCH BERSONDERS FAIRE, SAUBERE SPIELWEISE AUF.\0",
    // 28 (group 4, DS:6259)
    "DIE ZUSCHAUER SAHEN [USSERST FAIREN FUSSBALL BEIDER MANNSCHAFTEN.\0",
    // 29 (group 4, DS:629b)
    "BERSONDERS LOBENSWERT IST DIE SEHR FAIRE SPIELWEISE BESONDERS VON %a, DIE %x30IHREN EHRGEIZ NICHT MIT H[RTE VERWECHSELTEN#SICH BEI IHREM SIEG AUF SPIELERISCHE QUALIT[TEN BESANNEN#TROTZ DER NIEDERLAGE DIE NERVEN WAHRTEN#%.\0",
    // 30 (group 5, DS:6379)
    "%t %x02ZEIGTE SICH#WAR#% NACH DEM SPIEL %x40UNZUFRIEDEN#ZUFRIEDEN#% MIT DEM ERGEBNIS UND DEM SPIEL SEINER MANNSCHAFT.\0",
    // 31 (group 5, DS:63ef)
    "%t %x40WOLLTE SEINE%x03N UNMUT#N ZORN# WUT#% NICHT VERBERGEN#ZEIGTE SICHTLICHE ZUFRIEDENHEIT ]BER DAS %x02ERGEBNIS#SPIEL#%#%.\0",
    // 32 (group 5, DS:646d)
    "IN EINEM %x02INTERVIEW#GESPR[CH#% NACH %x02DEM SPIEL#DER PARTIE#% %x03MEINTE#SAGTE#ERL[UTERTE#% %t:%x40'ICH WERDE MICH MIT EINIGEN SPIELERN EINMAL UNTER VIER AUGEN UNTERHALTEN M]SSEN#ICH KANN EIGENTLICH ZUFRIEDEN SEIN MIT %x02DER LEISTUNG#DEM SPIEL#% MEINER %x03MANNSCHAFT#JUNGS#M[NNER#%#%.'\0",
    // 33 (group 5, DS:6591)
    "'%x40EINIGE SPIELER SCHIENEN NICHT MIT DER RICHTIGEN EINSTELLUNG AUF DEM PLATZ GEWESEN ZU SEIN#MAN KONNTE SEHEN, DASS MEINE MANNSCHAFT DURCHAUS GUTEN FUSSBALL SPIELEN KANN#%' %x02MEINTE#KOMMENTIERTE#% %t NACH DEM SPIEL.\0",
    // 34 (group 5, DS:666d)
    "%t %x02SAGTE#MEINTE#% NACH DEM SPIEL: %x40IN MEINER MANNSCHAFT WIRD SICH EINIGES [NDERN M]SSEN. AUF UNS WARTET WOHL NOCH VIEL ARBEIT#IN DIESER FORM MACHT ES SPASS, DER MANNSCHAFT ZUZUSEHEN, OBWOHL HIER UND DORT NOCH VERBESSERUNGEN DRIN SIND#%.'\0",
    // 35 (group 5, DS:6762)
    "KURZ NACH DEM SPIEL MEINTE %t: '%x40DAS WAR EIN TROSTLOSES SPIEL MEINER MANNSCHAFT, PERSONELLE VER[NDERUNGEN KANN ICH AUFGRUND SOLCH SCHWACHER LEISTUNGEN EINIGER SPIELER NICHT AUSSCHLIESSEN#DIE MANNSCHAFT HAT MEINE TAKTISCHEN ANWEISUNGEN SEHR GUT UMGESETZT UND GEZEIGT, DASS NOCH VIEL VON IHR ZU ERWARTEN IST#%.'\0",
    // 36 (group 5, DS:689b)
    "%t WAR VOM SPIEL SEINER MANNSCHAFT %x40SEHR ENTT[USCHT. 'ES WAR KEIN GROSSES AUFB[UMEN ZU VERSP]REN.#ANGETAN. 'EINIGES IST NOCH ZU VERBESSERN, ABER WENN WIR WEITER SO SPIELEN, SEHE ICH POSITIV IN DIE ZUKUNFT#% ', KOMMENTIERTE ER DAS SPIEL.\0",
    // 37 (group 6, DS:698b)
    "DER SCHATZMEISTER FREUTE SICH ]BER DAS AUSVERKAUFTE STADION.\0",
    // 38 (group 6, DS:69c8)
    "DIE AUSVERKAUFTEN R[NGE LASSEN GELD IN DIE KASSEN VON %a FLIESSEN.\0",
    // 39 (group 6, DS:6a0b)
    "DER ANBLICK DER VOLLEN R[NGE IM AUSVERKAUFTEN STADION ERFREUTE DENSCHATZMEISTER.\0",
    // 40 (group 6, DS:6a5c)
    "DER ANBLICK DES BIS AUF DIE LETZTEN PL[TZE GEF]LLTEN STADIONS MUSS DEMSCHATZMEISTER VON %a FREUDENTR[NEN IN DIE AUGEN GETRIEBEN HABEN.\0",
    // 41 (group 7, DS:6ae3)
    "DIE MEISTEN BESUCHER WAREN SICH EINIG, DASS DIESER SIEG %x50ABSOLUT GERECHTFERTIGT#VOLLKOMMEN UNVERDIENT#% WAR.\0",
    // 42 (group 7, DS:6b53)
    "DER SIEG VON %a WAR %x50VERDIENT#WIRKLICH NICHT VERDIENT#%.\0",
    // 43 (group 7, DS:6b8f)
    "AUCH EIN NEUTRALER BEOBACHTER DER PARTIE MUSS ZU DEM SCHLUSS KOMMEN, DASS DAS RESULTAT %x50MIT DEN GEZEIGTEN LEISTUNGEN ]BEREINSTIMMT#WOHL SEHR SCHMEICHELHAFT F]R DEN SIEGER IST#%.\0",
    // 44 (group 7, DS:6c44)
    "NACH DEM SPIELVERLAUF IST DAS %0:%1 ALS %x50DEN LEISTUNGEN BEIDER TEAMS ENTSPRECHEND#SEHR GL]CKLICH F]R DEN SIEGER#% ZU BEZEICHNEN.\0",
    // 45 (group 7, DS:6cc8)
    "ALLE WAREN SICH EINIG, DASS DIE PARTIE MIT %a EINEN %x50VERDIENTEN#GL]CKLICHEN#% SIEGER GEFUNDEN HAT.\0",
    // 46 (group 7, DS:6d2e)
    "DIE BESUCHER %x02DER PARTIE#DES SPIELS#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS DER %x02TRIUMPH#SIEG#% VON DER %x02KLAR#WESENTLICH#EINDEUTIG#% %x50BESSEREN#GL]CKLICHEREN#% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.\0",
    // 47 (group 7, DS:6e10)
    "%a WURDE VON %x02DEN ZUSCHAUERN#ALLEN BESUCHERN#% ALS %x50DER VERDIENTE#GL]CKLICHER#% %x30x#SIEGER#VERLIERER#% DIESER PARTIE %x50AKZEPTIERT#ANGESEHEN#%.\0",
    // 48 (group 8, DS:6ea9)
    "DIE HAUSHOHE ]BERLEGENHEIT DER %x10GASTGEBER#G[STE#% WAR FAST W[HREND DES GESAMTEN SPIELS NICHT ZU ]BERSEHEN.\0",
    // 49 (group 8, DS:6f17)
    "%a WAR%x03 DEM GEGNER#DER GEGNERISCHEN MANNSCHAFT# %b#% FAST ]BER DIE GESAMTE SPIEL%x02DAUER#ZEIT#% IN ALLEN %x02BELANGEN#BEREICHEN#% ]BERLEGEN.\0",
    // 50 (group 8, DS:6fa8)
    "DIE %x10PLATZHERREN#G[STE#% WAREN IN TOLLER %x02VERFASSUNG#SPIELLAUNE#% UND %x02BEHERRSCHTEN DEN GEGNER#BESTIMMTEN DAS SPIEL#% %x02SEHR SOUVER[N#FAST NACH BELIEBEN#%.\0",
    // 51 (group 8, DS:704f)
    "DIE %x02]BERDURCHSCHNITLICH GUTEN#ANSPRECHENDEN#% LEISTUNGEN %x02W[HREND DES GESAMTEN SPIELS#]BER DIE GESAMTE SPIELZEIT HINWEG#% MACHTEN %a ZUM VERDIENTEN SIEGER ]BER %b, DIE DEM %x03STURMLAUF#ANSTURM#ELAN#% DER %x10GASTGEBER#G[STE#% %x02SO GUT WIE#FAST#% NICHTS ENTGEGENZU%x02SETZEN#HALTEN#% HATTEN.\0",
    // 52 (group 8, DS:717c)
    "DEM %x03]BERLEGEN#DURCHDACHTEN#GERADLINIG#% UND %x03SOUVER[N#]BERZEUGEND#SCHNELL#% GEF]HRTEN SPIEL DER %x10PLATZHERREN#G[STE#% HATTEN DIE %x10G[STE#HAUSHERREN#% IN KEINER PHASE DES SPIELS ETWAS ENTGEGENZUSETZEN.\0",
    // 53 (group 8, DS:7250)
    "DEN SPIELERN VON %b WURDEN DURCH %a IHRE GRENZEN %x02GANZ KLAR#SEHR DEUTLICH#% AUFGEZEIGT. IN %x02VIELEN#EINIGEN#% PHASEN WURDEN DIE %x10G[STE#PLATZHERREN#% VORGEF]HRT WIE IM TRAINING.\0",
    // 54 (group 8, DS:7309)
    "DER SIEG VON %a WAR DAS %x02ERGEBNIS#RESULTAT#% EINER %x03TOLLEN#SEHR GUTEN#HOMOGENEN#% MANNSCHAFTSLEISTUNG.\0",
    // 55 (group 8, DS:7376)
    " %a WAR %x02DER GEGNERISCHEN MANNSCHAFT#DEM GEGNER#% IN PUNKTO KAMPF UND SPIELWITZ %x03UM L[NGEN ]BERLEGEN#EINIGES VORAUS#KLAR IM VORTEIL#%.\0",
    // 56 (group 9, DS:7403)
    "%a WAR DEM ]BERM[CHTIGEN GEGNER %b AN DIESEM TAG IN JEDER PHASE DES SPIELS UNTERLEGEN.\0",
    // 57 (group 9, DS:745a)
    "DIE %x10G[STE#GASTGEBER#% SCHN]RTEN %a FAST ]BER DIE GESAMTE SPIELZEIT IM DER EIGENEN H[LFTE EIN UND ERSPIELTEN SICH EINE CHANCE NACH DER ANDEREN.\0",
    // 58 (group 9, DS:74ed)
    "AM HEUTIGEN TAG HATTE DER %x10GASTGEBER#GAST#% SEINEM GEGNER NICHTS ENTGEGEN ZU SETZEN.\0",
    // 59 (group 9, DS:7545)
    "VON DER MANNSCHAFT VON %t HATTE DER GEGNER WEDER IN SPIELERISCHER NOCH IN K[MPFERISCHER ART ERNSTHAFT ETWAS ZU BEF]RCHTEN. ZU GROSS WAREN DIE LEISTUNGSUNTERSCHIEDE AM HEUTIGEN TAG.\0",
    // 60 (group 9, DS:75fa)
    "DIE SPIELER VON %t SCHLICHEN NACH DEM SPIEL VOM PLATZ. MAN WAR SICH EINIG, IM HEUTIGEN SPIEL DEM %x10GAST#GASTGEBER#% IN ALLEN PUNKTEN UNTERLEGEN GEWESEN ZU SEIN.\0",
    // 61 (group 9, DS:769d)
    "GROSSE LEISTUNGSUNTERSCHIEDE WAREN AUCH F]R DIE FANS VON %a NICHT ZU ]BERSEHEN, SO DASS SELBST DIE TREUESTEN ANH[NGER DIESE NIEDERLAGE ALS LEISTUNGSGERECHT ANSEHEN MUSSTEN.\0",
    // 62 (group 9, DS:774a)
    "%x02KEINER#NIEMAND#% H[TTE SICH BE%x02KLAGEN#SCHWEREN#% D]RFEN, WENN DIE NIEDERLAGE GEGEN %b NOCH %x02KLARER#DEUTLICHER#% AUSGEFALLEN W[RE.\0",
    // 63 (group 9, DS:77d6)
    "DIE %x02EXTREMEN#GROSSEN#% LEISTUNGSUNTERSCHIEDE %x02VON %a UND %b#BEIDER MANNSCHAFTEN#% WAREN %x02URSACHE#AUSSCHLAGGEBEND#% F]R DIE HEUTIGE NIEDERLAGE.\0",
    // 64 (group 9, DS:786f)
    "FUSSBALLEXPERTEN %x02DIE DAS SPIEL SAHEN#UNTER DEN ZUSCHAUERN#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS %x02DIE#DIESE#% NIEDERLAGE GEGEN %a %x02ABSOLUT#KLAR#% %x02GERECHTFERTIGT#VERDIENT#% WAR.\0",
    // 65 (group 9, DS:7934)
    "%x02%b#DER GEGNER#% WAR %x02WESENTLICH ST[RKER#]BERM[CHTIG STARK#% UND %x02IN JEDER PHASE#ZU JEDER ZEIT#% DIE %x03EINDEUTIG#DEUTLICH#KLAR#% %x03BESSERE#SPIELFREUDIGERE#DOMINIERENDE#% MANNSCHAFT.\0",
    // 66 (group 10, DS:79f7)
    "%c, DER HELD DES SPIELS MEINTE NACH DEM SPIEL ]BERGL]CKLICH:'%x04GEWONNEN HAT DAS GANZE TEAM, ICH BIN FROH, GUT GESPIELT ZU HABEN#ICH GLAUBE, DASS ICH GANZ GUT WAR#NUR WENN ALLE GUT SPIELEN, KANN MAN GEWINNEN#EIN TOLLES GEF]HL, SO GUT GESPIELT ZU HABEN#%.'\0",
    // 67 (group 10, DS:7af8)
    "%c ZEIGTE EINE ]BERRAGENDE LEISTUNG UND F]HRTE DIE MANNSCHAFT ZUM SIEG.\0",
    // 68 (group 10, DS:7b40)
    "BEFL]GELT VON %a'S %c'S SUPERLEISTUNG WUCHS DIE GESAMTE MANNSCHAFT ]BER SICH HINAUS.\0",
    // 69 (group 10, DS:7b95)
    "BESCHEIDEN GAB SICH DER SUPERSTAR DES SPIELS %c NACH SPIELENDE:%x04ICH DENKE OHNE DIE TOLLEN LEISTUNGEN ALLE ANDEREN, W[RE DER SIEG NIE M|GLICH GEWESEN#ZU EINEM SIEG BRAUCHT MAN ELF GUTE SPIELER, UND WIR HATTEN SIE HEUTE#DAS SPIEL LIEF F]R ALLE GUT, GLAUBE ICH. ICH BIN FROH GUT GESPIELT ZU HABEN#ICH HOFFE WIR SPIELEN IN DEN KOMMENDEN SPIELEN WEITER SO GUT#%.'\0",
    // 70 (group 11, DS:7cff)
    "%d WURDE NACH DEM SPIEL ALS DER VERLIERER DES TAGES DARGESTELLT, ABER AUCH SEINE MITSPIELER ZEIGTEN KEINE ANSPRECHENDEN LEISTUNGEN.\0",
    // 71 (group 11, DS:7d83)
    "IN EINEM INTERVIEW NACH DEM SPIEL MEINTE DER GROSSE VERLIERER %d:'%x03OKAY, ICH HATTE EINEN SCHWACHEN TAG, ABER ALLE WAREN NICHT SO GUT DRAUF#ICH MUSS VERSUCHEN IM N[CHSTEN SPIEL ALLES VERGESSEN ZU MACHEN#ICH HABE MICH BEI DER MANNSCHAFT SCHON F]R MEINE LEISTUNG ENTSCHULDIGT#%.'\0",
    // 72 (group 11, DS:7e9b)
    "%d BEF|RDERTE SEINE MANNSCHAFT DURCH SEIN SCHLECHTES SPIEL AUF DIE VERLIERERSTRASSE.\0",
    // 73 (group 11, DS:7ef0)
    "DER EIGENTLICHE VERLIERER DES SPIELS IST WOHL %d, DER MIT SEINEN AKTIONEN DAS SPIEL DER EIGENEN MANNSCHAFT ZERST|RTE, UND DURCH KATASTROPHALE FEHLP[SSE IMMER WIEDER F]R GEFAHR VOR DEM EIGENEN TOR SORGTE.\0",
    // 74 (group 12, DS:7fbc)
    "WAS DIE CHANCENAUSBEUTE BETRIFFT, SO KONNTE %a NICHT ]BERZEUGEN.\0",
    // 75 (group 12, DS:7ffd)
    "AN DER SCHWACHEN CHANCENVERWERTUNG SOLLTE NOCH ETWAS GEARBEITET WERDEN.\0",
    // 76 (group 12, DS:8045)
    "DIE CHANCEN H[TTEN KONSEQUENTER GENUTZT WERDEN SOLLEN.\0",
    // 77 (group 12, DS:807c)
    "%a SOLLTE SICH EINMAL ]BERLEGEN, EINEN SPIELER ZU VERPFLICHTEN, DER EINFACH NUR DIE B[LLE INS TOR TRITT.\0",
    // 78 (group 12, DS:80e5)
    "MIT EINER SOLCH SCHLECHTEN TORAUSBEUTE WIRD ES SCHWER SEIN, IN N[CHSTER ZEIT ERFOLGREICH ZU SEIN.\0",
    // 79 (group 12, DS:8147)
    "%a SOLLTE IM N[CHSTEN TRAINING EINMAL DAS VERWERTEN VON TORCHANCEN ]BEN, WILL MAN DIE N[CHSTEN SPIELE ERFOLGREICH BESTEITEN.\0",
    // 80 (group 12, DS:81c4)
    "WER SO VIELE TORCHANCEN VERGIBT, DARF SICH NICHT WUNDERN, WENN AM ENDE DER SAISON EINIGE PUNKTE WENIGER AUF DEM KONTO SIND, ALS MAN ERHOFFT HATTE.\0",
    // 81 (group 13, DS:8257)
    "DIE OPTIMALE CHANCENVERWERTUNG SPRICHT F]R %a.\0",
    // 82 (group 13, DS:8286)
    "MIT DIESER CHANCENVERWERTUNG WIRD %a IN DER N[CHSTEN ZEIT AUCH WEITERE SIEGE ERZIELEN.\0",
    // 83 (group 13, DS:82dd)
    "BEEINDRUCKEND WAR DIE SICHERE VERWERTUNG DER SICH BIETENDEN M|GLICHKEITEN.\0",
    // 84 (group 13, DS:8328)
    "DIE ABSOLUT KONSEQUENTE AUSNUTZUNG DER CHANCEN WAR DER GARANT F]R DIESEN SIEG GEGEN %b. F]R DIE ZUKUNFT L[SST DIES EINIGES ERHOFFEN.\0",
    // 85 (group 13, DS:83ad)
    "%a GL[NZTE DURCH TOLLE VERWERTUNG DER CHANCEN. DIE TREFFSICHERHEIT DER %x10GASTGEBER#G[STE#% WAR GERADEZU BE[NGSTIGEND F]R DIE KOMMENDEN GEGNER.\0",
    // 86 (group 13, DS:843e)
    "%a ZEIGTE, DASS EINE KONSEQUENTE AUSNUTZUNG DER CHANCEN DER GRUNDSTEIN F]R EINEN SIEG IST.\0",
    // 87 (group 14, DS:8499)
    "%a BEWIES STARKE MORAL UND WAR DURCH DEN %2:%3 R]CKSTAND NICHT ZU BEEINDRUCKEN.\0",
    // 88 (group 14, DS:84e9)
    "%a ERHOLTE SICH SCHNELL VON DEM %2:%3 R]CKSTAND UND GELANG ]BER DEN KAMPF ZU GUTEM SPIEL.\0",
    // 89 (group 14, DS:8543)
    "DIE TOLLE AUFHOLJAGD VON %a BEEINDRUCKTE SOGAR DIE GEGNERISCHEN FANS.\0",
    // 90 (group 14, DS:8589)
    "MIT RIESIGEM BEIFALL BEDANKTEN SICH DIE FANS F]R DIE LEISTUNGSSTEIGERUNG IHRER MANNSCHAFT, DIE EIN %2:%3 AUFHOLTE.\0",
    // 91 (group 14, DS:85fc)
    "%a BEEINDRUCKTE NICHT NUR DIE EIGENEN FANS MIT DER TOLLEN AUFHOLJAGD. ES GELANG, EINEN %2:%3 R]CKSTAND NOCH AUFZUHOLEN.\0",
    // 92 (group 14, DS:8674)
    "DIE MORAL VON %a SCHEINT NICHT ZU BRECHEN ZU SEIN, DER MANNSCHAFT GELANG ES, AUS EINEM %2:%3 NOCH EIN %0:%1 ZU MACHEN.\0",
    // 93 (group 15, DS:86eb)
    "%t ZEIGTE SICH ERBOSST: ES MUSS DOCH AUCH EINMAL M|GLICH SEIN, EINE %4:%5 F]HRUNG ZU VERTEIDIGEN.\0",
    // 94 (group 15, DS:874d)
    "%a ZEIGTE NERVEN UND LIESS SICH EINE %4:%5 F]HRUNG NOCH NEHMEN.\0",
    // 95 (group 15, DS:878d)
    "ENTT[USCHT [USSERTEN SICH DIE FANS VON %a DAR]BER, WIE LEICHTSINNIG EINE %4:%5 F]HRUNG VON IHRER MANNSCHAFT VERSPIELT WURDE.\0",
    // 96 (group 15, DS:880a)
    "ES DARF EINER MANNSCHAFT EIGENTLICH NICHT PASSIEREN, SICH EINE %4:%5 F]HRUNG NOCH NEHMEN ZU LASSEN.\0",
    // 97 (group 15, DS:886e)
    "SEHR VER[RGERT ZEIGTE SICH %t NACH DEM SPIEL. ER BEM[NGELTE, DASS DER %4:%5 VORSPRUNG NICHT VERTEIDIGT WERDEN KONNTE.\0",
    // 98 (group 16, DS:88e4)
    "DER %x02SCHNELLE#FR]HE#% R]CKSTAND WURDE VON %a %x30RECHT GUT WEGGESTECKT#AUSGEZEICHNET VERKRAFTET#NICHT VERKRAFTET#%, WAS VOR ALLEM AUF %x30DIE K[MPFERISCHE EINSTELLUNG#TOLLE LEISTUNG DER GESAMTEN MANNSCHAFT#FEHLENDE MORAL DES TEAMS#% ZUR]CKZUF]HREN IST.\0",
    // 99 (group 17, DS:89e4)
    "DIE %x03SCHNELLE F]HRUNG#FR]HE F]HRUNG#F]HRUNG IN DEN ERSTEN MINUTEN#% F]HRTE ZU%x30 L[SSIGKEITEN IN DER ABWEHR# SICHERHEIT IN ALLEN KOMBINATIONEN. ES GELANG VIELES#M UNTERSCH[TZEN DES GEGNERS, DER IN DER FOLGEZEIT SEHR VON DER NACHL[SSIGEN SPIELWEISE PROFITIERTE#%.\0",
    // 100 (group 18, DS:8aef)
    "%x30%x02DAS UNENTSCHIEDEN#DIE PUNKTETEILUNG#%#DER %x02ERFOLG#SIEG#%#DIE NIEDERLAGE#% GEGEN %b KAM ERST IN DEN %x02SCHLUSSMINUTEN#LETZTEN MINUTEN#% ZUSTANDE, MUSS %x02DAHER#AUS DIESER SICHT#% WOHL ALS %x30ERW[HNENSWERT#SEHR GL]CKLICH#SEHR [RGERLICH#% BEZEICHNET WERDEN.\0",
    // 101 (group 18, DS:8bfc)
    "BESONDERS %x30ERW[HNENSWERT#GL]CKLICH#[RGERLICH#% WAR, DASS %x30%x02DAS UNENTSCHIEDEN#DIE PUNKTETEILUNG#%#DER %x02ERFOLG#SIEG#%#DIE NIEDERLAGE#% GEGEN %b ERST IN DEN %x02SCHLUSSMINUTEN#LETZTEN MINUTEN#% ZUSTANDE KAM.\0",
    // 102 (group 19, DS:8cd5)
    "DIE MEISTEN BESUCHER WAREN SICH EINIG, DASS DIESE NIEDERLAGE %x50VOLLKOMMEN UNVERDIENT#ABSOLUT GERECHTFERTIGT#% WAR.\0",
    // 103 (group 19, DS:8d4a)
    "DIE NIEDERLAGE VON %a WAR %x50WIRKLICH NICHT VERDIENT#VERDIENT#%.\0",
    // 104 (group 19, DS:8d8c)
    "AUCH EIN NEUTRALER BEOBACHTER DER PARTIE MUSS ZU DEM SCHLUSS KOMMEN, DASS DAS RESULTAT %x50WOHL SEHR SCHMEICHELHAFT F]R DEN SIEGER IST#MIT DEN GEZEIGTEN LEISTUNGEN ]BEREINSTIMMT#%.\0",
    // 105 (group 19, DS:8e41)
    "NACH DEM SPIELVERLAUF IST DAS %0:%1 ALS %x50SEHR GL]CKLICH F]R DEN SIEGER#DEN LEISTUNGEN BEIDER TEAMS ENTSPRECHEND#% ZU BEZEICHNEN.\0",
    // 106 (group 19, DS:8ec5)
    "ALLE WAREN SICH EINIG, DASS DIE PARTIE MIT %b EINEN %x50GL]CKLICHEN#VERDIENTEN#% SIEGER GEFUNDEN HAT.\0",
    // 107 (group 19, DS:8f2b)
    "DIE BESUCHER %x02DER PARTIE#DES SPIELS#% WAREN %x02EINER MEINUNG#SICH EINIG#%, DASS DER SIEG VON DER %x02KLAR#WESENTLICH#EINDEUTIG#% %x50GL]CKLICHEREN#BESSEREN#% MANNSCHAFT %x03ERZIELT#ERRUNGEN#ERREICHT#% WURDE.\0",
    // 108 (group 19, DS:8fff)
    "%a WURDE VON %x02DEN ZUSCHAUERN#ALLEN BESUCHERN#% ALS %x50UNGL]CKLICHER#VERDIENTER#% VERLIERER DIESER PARTIE ANGESEHEN.\0",
    // 109 (group 20, DS:9077)
    "DIES IST HAUS NUMMER 3 (DONNERSTAGE WAREN PROBLEMTAGE F]R IHN).\0",
)
.as_bytes();

/// DS:3e49.. the 69 headline templates (pointer table DS:4f90), laid out like
/// [`BODY_IMAGE`]. Every headline holds a `%e` that ends its first line.
pub(super) const HEADLINE_IMAGE: &[u8] = concat!(
    // 0 (group 0, DS:3e49)
    "%a }BERROLLT%e%b MIT %0:%1\0",
    // 1 (group 0, DS:3e64)
    "%a %x02DEMORALISIERT#DEKLASSIERT#%%e%b MIT %0:%1\0",
    // 2 (group 0, DS:3e95)
    "%a FERTIGT%e%b MIT %0:%1 AB\0",
    // 3 (group 0, DS:3eb1)
    "%a MACHT %x03KLASSE #SUPER #RIESEN#%SPIEL%e%x10GEGEN#IN#% %b\0",
    // 4 (group 1, DS:3eee)
    "%a %x02SCHL[GT#BESIEGT#%%e%b KNAPP MIT %0:%1\0",
    // 5 (group 1, DS:3f1b)
    "%0:%1 ZITTERSIEG%e%x10GEGEN#IN#% %b\0",
    // 6 (group 2, DS:3f3f)
    "%a BESIEGT%e%b MIT %0:%1\0",
    // 7 (group 2, DS:3f58)
    "%a SCHL[GT%e%b %0:%1\0",
    // 8 (group 2, DS:3f6d)
    "%a GEWINNT%e%x10GEGEN#IN#% %b %0:%1\0",
    // 9 (group 3, DS:3f91)
    "%a WIRD VON%e%b %0:%1 %xDEKLASSIERT#]BERROLLT#%\0",
    // 10 (group 3, DS:3fc1)
    "%0:%1 PLEITE %x10GEGEN#IN#%%e%b\0",
    // 11 (group 3, DS:3fe1)
    "%0:%1 PACKUNG F]R %a%e%x10GEGEN#IN#% %b\0",
    // 12 (group 3, DS:4009)
    "BITTERE %0:%1 %x02NIEDERLAGE#SCHLAPPE#%%e%x10GEGEN#IN#% %b\0",
    // 13 (group 4, DS:4044)
    "%a %x02VERLIERT %x10GEGEN#IN#%#UNTERLIEGT#%%e%b KNAPP MIT %0:%1\0",
    // 14 (group 4, DS:4084)
    "KNAPPE %0:%1 NIEDERLAGE%e%x10GEGEN#IN#% %b\0",
    // 15 (group 5, DS:40af)
    "%a UNTERLIEGT%e%b MIT %0:%1\0",
    // 16 (group 5, DS:40cb)
    "%a %x10GEGEN#IN#%%e%b MIT %0:%1 UNTERLEGEN\0",
    // 17 (group 5, DS:40f6)
    "%a VERLIERT%e%x10GEGEN#IN#% %b %0:%1\0",
    // 18 (group 6, DS:411b)
    "%a UND%e%b TRENNEN SICH %0:%1\0",
    // 19 (group 7, DS:4139)
    "%0:%1 %x10GEGEN#IN#% %b%eVOR AUSVERKAUFTEM HAUS\0",
    // 20 (group 7, DS:4169)
    "%9 ZUSCHAUER SAHEN%eEIN %0:%1 %x10GEGEN#IN#% %b\0",
    // 21 (group 8, DS:4199)
    "%a MACHT AUS %2:%3%eNOCH EIN %0:%1\0",
    // 22 (group 8, DS:41bc)
    "%2:%3 %x02TOLL#GUT#% %x02VERKRAFTET#WEGGESTECKT#%-%e%0:%1 %x10GEGEN#IN#% %b\0",
    // 23 (group 8, DS:4208)
    "NACH %x03GROSSER#TOLLER#RIESEN#% LEISTUNGSSTEIGERUNG%e%0:%1 %x10GEGEN#IN#% %b\0",
    // 24 (group 8, DS:4256)
    "%a SCHAFFT NOCH%eWENDE %x10GEGEN#IN#% %b\0",
    // 25 (group 9, DS:427f)
    "TROTZ %4:%5 %x02VORSPRUNG#F]HRUNG#% %0:%1%e%x10GEGEN#IN#% %b\0",
    // 26 (group 9, DS:42bc)
    "%a VERLIERT %0:%1 NACH%e%4:%5 %x02VORSPRUNG#F]HRUNG#% %x10GEGEN#IN#% %b\0",
    // 27 (group 9, DS:4304)
    "%0:%1-PLEITE NACH %4:%5%eF]HRUNG %x10GEGEN#IN#% %b\0",
    // 28 (group 10, DS:4337)
    "NUR %0:%1 %x10GEGEN#BEI#% %b%eTROTZ %4:%5 F]HRUNG\0",
    // 29 (group 10, DS:4369)
    "TROTZ %4:%5 KEIN SIEG%e%x10GEGEN#BEI#% %b\0",
    // 30 (group 11, DS:4393)
    "TOLL GEK[MPFT, AUS %2:%3%eEIN %0:%1 GEMACHT\0",
    // 31 (group 11, DS:43bf)
    "%a ERREICHTE NACH %2:%3%eNOCH %0:%1 %x10GEGEN#BEI#% %b\0",
    // 32 (group 12, DS:43f6)
    "%c SCHOSS%e%a ZUM SIEG\0",
    // 33 (group 12, DS:440d)
    "%c ERSCHOSS%e%b\0",
    // 34 (group 12, DS:441d)
    "%0:%1 DANK EINES%e]BERRAGENDEN %c\0",
    // 35 (group 12, DS:443f)
    "%c MACHT RIESENSPIEL%e%x10GEGEN#BEI#% %b\0",
    // 36 (group 12, DS:4468)
    "%c BESTER SPIELER%e%x10GEGEN#BEI#% %b\0",
    // 37 (group 12, DS:448e)
    "%a SIEGT DANK%eWELTKLASSELEISTUNG VON %c\0",
    // 38 (group 12, DS:44b7)
    "%c GARANT F]R DEN SIEG%e%x10GEGEN#BEI#% %b\0",
    // 39 (group 13, DS:44e2)
    "TOLLES %0:%1 IM SPIEL %a%e%x10GEGEN#BEI#% %b\0",
    // 40 (group 13, DS:450f)
    "TOLLES SPIEL VON %a%eUND %b - %0:%1\0",
    // 41 (group 13, DS:4533)
    "%0:%1 %x10GEGEN#BEI#% %b-%eEIN SUPERSPIEL\0",
    // 42 (group 13, DS:455d)
    "TOLLES FUSSBALLSPIEL%e%0:%1 %x10GEGEN#BEI#% %b\0",
    // 43 (group 14, DS:458c)
    "LANGWEILIGES %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 44 (group 14, DS:45b3)
    "ZUSCHAUER VER[RGERT ]BER%eLANGWEILIGES %0:%1\0",
    // 45 (group 14, DS:45e0)
    "M]DES GECKICKE IM SPIEL%e %x10GEGEN#BEI#% %b\0",
    // 46 (group 14, DS:460d)
    "GRAUENHAFTES BALLGESCHIEBE%eZUSCHAUER PFIFFEN TEAMS AUS\0",
    // 47 (group 14, DS:4645)
    "EIN TRAUERSPIEL%eIN ZWEI AKTEN - %0:%1\0",
    // 48 (group 15, DS:466c)
    "%d DER S]NDENBOCK%e%x10GEGEN#BEI#% %b\0",
    // 49 (group 15, DS:4692)
    "SCHWACHE LEISTUNG%eVON %d\0",
    // 50 (group 15, DS:46ac)
    "RABENSCHWARZER TAG F]R%e%d %x10GEGEN#BEI#% %b\0",
    // 51 (group 16, DS:46da)
    "UNGL]CKLICHES %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 52 (group 16, DS:4702)
    "VIEL PECH BEIM %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 53 (group 16, DS:472b)
    "%a VERZWEIFELT AN DER%eABWEHR VON %b\0",
    // 54 (group 17, DS:4750)
    "%x02SCHMEICHELHAFTES#GL]CKLICHES#% %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 55 (group 17, DS:478d)
    "ABWEHR VON %a%eSTAND %x02GUT#SICHER#%\0",
    // 56 (group 17, DS:47b3)
    "VIEL GL]CK BEIM %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 57 (group 18, DS:47dd)
    "HOCHVERDIENTES %0:%1%e%x10GEGEN#BEI#% %b\0",
    // 58 (group 18, DS:4806)
    "%a MIT SUPERSPIEL%e%0:%1 %x10GEGEN#BEI#% %b\0",
    // 59 (group 18, DS:4832)
    "SOUVER[NER %0:%1%eERFOLG %x10GEGEN#BEI#% %b\0",
    // 60 (group 19, DS:485e)
    "%a OHNE CHANCE IM SPIEL%e%x10GEGEN#BEI#% %b\0",
    // 61 (group 19, DS:488a)
    "%b DIE KLAR BESSERE%eMANNSCHAFT\0",
    // 62 (group 19, DS:48aa)
    "VERDIENTE %0:%1 NIEDERLAGE%eF]R %a\0",
    // 63 (group 20, DS:48cd)
    "LEISTUNGSGERECHTES %0:%1 IM SPIEL%e%x10GEGEN#BEI#% %b\0",
    // 64 (group 20, DS:4903)
    "%0:%1 IN AUSGEGLICHENEM SPIEL%eTEAMS WAREN MIT EINEM PUNKT ZUFRIEDEN\0",
    // 65 (group 21, DS:4948)
    "GL]CKLICHES %0:%1 F]R %a%e%x10GEGEN#BEI#% %b\0",
    // 66 (group 21, DS:4975)
    "PUNKT MIT VIEL GL]CK%e%0:%1 %x10GEGEN#BEI#% %b\0",
    // 67 (group 22, DS:49a4)
    "NUR %0:%1 TROTZ KLARER%e]BERLEGENHEIT %x10GEGEN#BEI#% %b\0",
    // 68 (group 22, DS:49dd)
    "%0:%1 - PUNKT VERSCHENKT%e%a NUTZTE CHANCEN NICHT AUS\0",
)
.as_bytes();

/// Start of each body template in [`BODY_IMAGE`].
pub(super) const BODY_OFFSETS: [u16; 110] = template_offsets(BODY_IMAGE);
/// Start of each headline template in [`HEADLINE_IMAGE`].
pub(super) const HEADLINE_OFFSETS: [u16; 69] = template_offsets(HEADLINE_IMAGE);

/// Offsets of the first `N` NUL-separated strings of `image`.
const fn template_offsets<const N: usize>(image: &[u8]) -> [u16; N] {
    let mut out = [0u16; N];
    let mut k = 1;
    let mut i = 0;
    while k < N {
        if image[i] == 0 {
            out[k] = (i + 1) as u16;
            k += 1;
        }
        i += 1;
    }
    out
}

/// DS:9350 number of body templates per group. Groups 20..24 are never
/// picked; only template 109 exists of group 20.
pub(super) const BODY_GROUP_SIZES: [i16; 25] = [
    8, 5, 4, 8, 5, 7, 4, 7, 8, 10, 4, 4, 7, 6, 6, 5, 1, 1, 2, 7, 3, 2, 2, 3, 4,
];

/// DS:9322 number of headline templates per group.
pub(super) const HEADLINE_GROUP_SIZES: [i16; 23] = [
    4, 2, 3, 4, 2, 3, 1, 2, 4, 3, 2, 2, 7, 4, 5, 3, 3, 3, 3, 3, 2, 2, 2,
];

/// DS:937e weight of a goal or chance for the minutes `t - 5 ..= t + 5`.
pub(super) const HEAT_WEIGHTS: [i16; 11] = [3, 4, 5, 7, 9, 10, 9, 7, 5, 4, 3];

/// DS:4bc4[0] (-> DS:2fc2) label of the goal line of the sidebar.
pub(super) const SIDEBAR_GOALS: &[u8] = b"TORE: ";
/// DS:4bc4[1] the goal line without goals.
pub(super) const SIDEBAR_NO_GOALS: &[u8] = b"FEHLANZEIGE";
/// DS:4bc4[2] label of the yellow-card line.
pub(super) const SIDEBAR_YELLOW: &[u8] = b"GELBE KARTEN: ";
/// DS:4bc4[3] a card line without cards.
pub(super) const SIDEBAR_NONE: &[u8] = b"KEINE";
/// DS:4bc4[4] label of the red-card line.
pub(super) const SIDEBAR_RED: &[u8] = b"ROTE KARTEN  : ";

/// DS:267c[0..6] message templates: `$` takes a text argument, `#` a number,
/// `^` is the line break.
pub(super) const MESSAGE_TEMPLATES: [&[u8]; 6] = [
    b"$ bietet an,^von # auf #^Jahre zu verl{ngern.^",
    b"$^ist an $^interessiert.^",
    b"$^$^$ ^",
    b"$ h{ngt den^Fu~balljob im Alter von^# Jahren an den Nagel.^",
    b"$ k}ndigt an,^da~ er seinen Vertrag nicht^mehr verl{ngern wird.^",
    b"$^$^$ ^",
];

/// DS:23fc month names.
pub(super) const MONTH_NAMES: [&[u8]; 12] = [
    b"Januar",
    b"Februar",
    b"M{rz",
    b"April",
    b"Mai",
    b"Juni",
    b"Juli",
    b"August",
    b"September",
    b"Oktober",
    b"November",
    b"Dezember",
];

/// DS:07b8 month lengths (February is always 28).
pub(super) const MONTH_LENGTHS: [i8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

#[cfg(test)]
mod tests {
    use super::*;

    /// Load segment of DGROUP in the unpacked `BMMAIN.EXE` image.
    const DGROUP_SEGMENT: usize = 0x4cb3;
    /// DS address of [`BODY_IMAGE`].
    const BODY_BASE: usize = 0x5558;
    /// DS address of [`HEADLINE_IMAGE`].
    const HEADLINE_BASE: usize = 0x3e49;

    fn templates(image: &'static [u8], offsets: &[u16]) -> Vec<&'static [u8]> {
        offsets
            .iter()
            .map(|&o| {
                let s = &image[usize::from(o)..];
                &s[..s.iter().position(|&c| c == 0).unwrap()]
            })
            .collect()
    }

    #[test]
    fn template_tables_are_consistent() {
        for (image, offsets) in [
            (BODY_IMAGE, &BODY_OFFSETS[..]),
            (HEADLINE_IMAGE, &HEADLINE_OFFSETS[..]),
        ] {
            assert_eq!(offsets[0], 0);
            for &o in &offsets[1..] {
                assert_eq!(image[usize::from(o) - 1], 0);
            }
            assert_eq!(image.last(), Some(&0));
            let last = usize::from(*offsets.last().unwrap());
            assert_eq!(image[last..].iter().filter(|&&c| c == 0).count(), 1);
        }
        assert_eq!(HEADLINE_GROUP_SIZES.iter().sum::<i16>(), 69);
        assert_eq!(BODY_GROUP_SIZES[..20].iter().sum::<i16>(), 109);
        assert!(
            templates(HEADLINE_IMAGE, &HEADLINE_OFFSETS)
                .iter()
                .all(|t| t.windows(2).any(|w| w == b"%e"))
        );
    }

    #[test]
    fn template_codes_are_known() {
        let all = templates(BODY_IMAGE, &BODY_OFFSETS)
            .into_iter()
            .chain(templates(HEADLINE_IMAGE, &HEADLINE_OFFSETS));
        for (k, t) in all.enumerate() {
            let mut i = 0;
            while i < t.len() {
                let end_marker = i > 0 && t[i - 1] == b'#';
                if t[i] == b'%' && !end_marker {
                    let code = t[i + 1];
                    if code == b'x' {
                        let n = t[i + 2];
                        assert!(
                            n == b'0' || (b'1'..=b'5').contains(&n) || n == b'D',
                            "template {k}: %x{}",
                            n as char
                        );
                        i += 4;
                        continue;
                    }
                    assert!(
                        b"0123459abcdte".contains(&code),
                        "template {k}: %{}",
                        code as char
                    );
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
    }

    #[test]
    fn real_exe_tables_match() {
        let Some(dir) = bmp_formats::data_dir() else {
            return;
        };
        let Ok(exe) = std::fs::read(dir.join("BMMAIN.EXE")) else {
            return;
        };
        let image = bmp_formats::exepack::unpack(&exe).unwrap();
        let ds = &image[DGROUP_SEGMENT * 16..];
        let word = |a: usize| u16::from_le_bytes([ds[a], ds[a + 1]]);
        let int = |a: usize| word(a) as i16;
        let far_ptr = |a: usize| {
            assert_eq!(
                usize::from(word(a + 2)),
                DGROUP_SEGMENT,
                "segment at DS:{a:04x}"
            );
            usize::from(word(a))
        };
        let c_str = |a: usize| {
            let s = &ds[a..];
            &s[..s.iter().position(|&c| c == 0).unwrap()]
        };

        assert_eq!(&ds[BODY_BASE..BODY_BASE + BODY_IMAGE.len()], BODY_IMAGE);
        assert_eq!(
            &ds[HEADLINE_BASE..HEADLINE_BASE + HEADLINE_IMAGE.len()],
            HEADLINE_IMAGE
        );
        for (i, &o) in BODY_OFFSETS.iter().enumerate() {
            assert_eq!(
                far_ptr(0x90d2 + 4 * i),
                BODY_BASE + usize::from(o),
                "body {i}"
            );
        }
        for (i, &o) in HEADLINE_OFFSETS.iter().enumerate() {
            assert_eq!(
                far_ptr(0x4f90 + 4 * i),
                HEADLINE_BASE + usize::from(o),
                "headline {i}"
            );
        }
        for (i, &v) in BODY_GROUP_SIZES.iter().enumerate() {
            assert_eq!(int(0x9350 + 2 * i), v);
        }
        for (i, &v) in HEADLINE_GROUP_SIZES.iter().enumerate() {
            assert_eq!(int(0x9322 + 2 * i), v);
        }
        for (i, &v) in HEAT_WEIGHTS.iter().enumerate() {
            assert_eq!(int(0x937e + 2 * i), v);
        }
        for (i, &v) in MONTH_LENGTHS.iter().enumerate() {
            assert_eq!(ds[0x07b8 + i] as i8, v);
        }
        for (i, name) in MONTH_NAMES.iter().enumerate() {
            assert_eq!(c_str(far_ptr(0x23fc + 4 * i)), *name);
        }
        for (i, tpl) in MESSAGE_TEMPLATES.iter().enumerate() {
            assert_eq!(c_str(far_ptr(0x267c + 4 * i)), *tpl);
        }
        let sidebar = [
            SIDEBAR_GOALS,
            SIDEBAR_NO_GOALS,
            SIDEBAR_YELLOW,
            SIDEBAR_NONE,
            SIDEBAR_RED,
        ];
        for (i, s) in sidebar.iter().enumerate() {
            assert_eq!(c_str(far_ptr(0x4bc4 + 4 * i)), *s);
        }
    }
}

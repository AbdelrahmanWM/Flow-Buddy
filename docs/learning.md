# How Flow Buddy teaches

Flow Buddy teaches Vim while you work: it notices habits, suggests better ways, and introduces one new thing a day. This page says what that rests on, so each part can be checked against the evidence and changed when the evidence says so.

## What the research says

| Finding | Source | What Flow Buddy does with it |
|---|---|---|
| Memory decays, and reviewing just before you'd forget works best; a learner's own data can predict when that is. | Settles & Meeder, [A Trainable Spaced Repetition Model for Language Learning](https://aclanthology.org/P16-1174/), ACL 2016 (half-life regression; [code](https://www.github.com/duolingo/halflife-regression)); Cepeda et al., [Distributed practice in verbal recall tasks](https://pubmed.ncbi.nlm.nih.gov/16719566/), *Psychological Bulletin* 132(3), 2006 | Each skill has a half-life, fitted to your own gaps between uses; a skill comes back for review when its predicted recall drops. |
| Whether a skill is learned can be estimated from the evidence of using it right or not. | Corbett & Anderson, [Knowledge tracing: Modeling the acquisition of procedural knowledge](https://dblp.dagstuhl.de/rec/journals/umuai/CorbettA95.html), *User Modeling and User-Adapted Interaction* 4(4), 1995; Pelánek, [Applications of the Elo rating system in adaptive educational systems](https://is.muni.cz/publication/1376317), *Computers & Education* 98, 2016 | Each skill has a mastery probability: using it is evidence for, falling back on the habit it replaces is evidence against. |
| Learning is fastest at a set difficulty: about 85% success. | Wilson, Shenhav, Straccia & Cohen, [The Eighty Five Percent Rule for optimal learning](https://pmc.ncbi.nlm.nih.gov/articles/PMC6831579), *Nature Communications* 10, 2019 | Lessons and tips aim at skills you're likely, not certain, to pick up: one step past what you use. |
| Producing a skill strengthens it more than reading about it. | Roediger & Karpicke, [Test-Enhanced Learning](https://psychology.ecu.edu/wp-content/pv-uploads/sites/216/2019/03/Roediger-Karpicke-2006.pdf), *Psychological Science* 17(3), 2006 | Challenges: "use `ciw` the next time you change a word", checked against what you then do. |
| Mixing related skills beats practising one at a time. | Rohrer & Taylor, [The shuffling of mathematics problems improves learning](https://digitalcommons.usf.edu/psy_facpub/1767/), *Instructional Science* 35(6), 2007 | Reviews and challenges mix skills rather than repeating one. |
| Skill grows with practice along a smooth curve; per person it's better fitted by an exponential than a power law. | Newell & Rosenbloom, [Mechanisms of skill acquisition and the law of practice](https://aitopics.org/doc/classics:7D0BECCD), 1981; Heathcote, Brown & Mewhort, 2000 (via [Refining the Law of Practice](https://figshare.utas.edu.au/articles/journal_contribution/Refining_the_Law_of_Practice/22964183/1/files/40706153.pdf)) | Mastery and half-life are fitted per person, not from an average curve. |
| Prerequisites among skills can be inferred from data, but data alone is often inconclusive; controlled comparisons settle it. | Scheines, Silver & Goldin, [Discovering Prerequisite Relationships among Knowledge Components](https://www.stat.cmu.edu/~brian/nynke/726-2021/week06/HCI%20prereq%20discovery/ScheinesSilverGoldin-EDM2014.pdf), EDM 2014; [WPI dissertation](https://digital.wpi.edu/downloads/bz60cw54g?locale=en) | The hand-made prerequisite graph is a prior; your own order of learning strengthens or weakens each link. |
| Whether a change helps is only known by comparing against not making it. | Kohavi, Tang & Xu, *Trustworthy Online Controlled Experiments*, Cambridge University Press, 2020 | A small share of tips is held back at random, so their effect can be measured. |

None of this research is about learning text editors: it's about memory, skills and practice in general. So these choices are a starting point, and Flow Buddy measures whether they work for you (`flowbuddy learn --report`).

## Reviews

A mastered skill comes back as the day's lesson ("Remember this?") once the chance you'd recall it drops under `learn.review_at` (70% by default), worked out from its half-life: the days for that chance to halve, which grows each time you use it after a gap. The review says how long it's been ("You haven't used `ci(` in 21 days"). Reviews and new lessons compete in one ranking, but reviews never outnumber new lessons over a week by more than one. `flowbuddy learn --json` gives each skill's `recall` and `half_life_days`, and the skill map shows the ones slipping.

The other way to practise is on purpose: `flowbuddy practice` drills the habit you trip on most, and `learn --report` shows how drills have gone next to how tips have.

## What it covers

The inventory starts from Vim's own command index (`:help index`, `runtime/doc/index.txt`): every command, by mode. `flowbuddy learn --coverage` shows how much of it the curriculum teaches and how much a rule can coach.

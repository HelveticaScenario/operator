// Party example: a four-on-the-floor beat with video behind the code.
// Press Alt+Enter to play, Alt+. to stop. Then try the 🐨 / 📈 / 🤠 buttons.

$setTempo(124);

// Kick on every beat: a sine that drops in pitch as its envelope decays.
const kick = $perc($clock.beatTrigger, { decay: 0.3 });
$sine(kick.range('45hz', '180hz')).amplitude(kick).out();

// Off-beat hats.
const hats = $cycle($p('~ c4 ~ c4 ~ c4 ~ c4'));
$noise('white').amplitude($perc(hats.trig, { decay: 0.04 })).amplitude(1.5).out();

// A bass line through a plucked lowpass.
const bass = $cycle($p('c2 c2 eb2 <g1 bb1>'));
const pluck = $adsr(bass.gate, { attack: 0.005, decay: 0.2, sustain: 0.3, release: 0.1 });
$lpf($saw(bass), pluck.range('120hz', '1800hz'), 2).amplitude(3).out();

// Video mode: a spinning kaleidoscope.
$v.osc(10).$.kaleid(6).$.hsv().out();

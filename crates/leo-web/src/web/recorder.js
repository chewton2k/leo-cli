(function (root) {
  'use strict';

  const RATE = 16000;
  const CHUNK = 1600;

  class Downsampler {
    constructor(inputRate, outputRate = RATE) {
      this.step = inputRate / outputRate;
      this.phase = 0;
      this.sum = 0;
      this.count = 0;
      this.out = [];
    }

    push(channels) {
      const first = channels[0];
      if (!first) return [];
      const made = [];
      const width = channels.length;
      for (let i = 0; i < first.length; i++) {
        let v = 0;
        for (let c = 0; c < width; c++) v += channels[c][i];
        this.sum += v / width;
        this.count += 1;
        this.phase += 1;
        if (this.phase >= this.step) {
          this.phase -= this.step;
          const s = Math.max(-1, Math.min(1, this.sum / this.count));
          made.push(s < 0 ? Math.round(s * 32768) : Math.round(s * 32767));
          this.sum = 0;
          this.count = 0;
        }
      }
      return made;
    }
  }

  function peak(samples) {
    let most = 0;
    for (const s of samples) most = Math.max(most, Math.abs(s));
    return most / 32768;
  }

  root.leoRecorder = { Downsampler, peak, RATE, CHUNK };

  if (typeof root.registerProcessor === 'function') {
    class LeoRecorder extends root.AudioWorkletProcessor {
      constructor(options) {
        super();
        this.dual = Boolean(options.processorOptions && options.processorOptions.dual);
        this.other = new Downsampler(root.sampleRate);
        this.down = new Downsampler(root.sampleRate);
        this.held = [];
        this.port.onmessage=(event) => {
          if (!event.data.flush) return;
          if (this.held.length) { const chunk=Int16Array.from(this.held); this.held=[]; this.port.postMessage({samples:chunk.buffer,peak:peak(chunk)},[chunk.buffer]); }
          this.port.postMessage({flushed:true});
        };
      }

      process(inputs) {
        const input = inputs[0];
        if (input && input.length) {
          if (this.dual) {
            const left = this.down.push([input[0]]);
            const right = this.other.push([input[1] || new Float32Array(input[0].length)]);
            for (let i=0; i<left.length; i++) this.held.push(left[i],right[i]);
          } else for (const s of this.down.push(input)) this.held.push(s);
          while (this.held.length >= CHUNK * (this.dual ? 2 : 1)) {
            const chunk = Int16Array.from(this.held.splice(0, CHUNK * (this.dual ? 2 : 1)));
            this.port.postMessage({ samples: chunk.buffer, peak: peak(chunk) }, [chunk.buffer]);
          }
        }
        return true;
      }
    }
    root.registerProcessor('leo-recorder', LeoRecorder);
  }
})(globalThis);

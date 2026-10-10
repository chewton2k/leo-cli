(function(root) {
  'use strict';
  const MOST_CHUNKS=100000;
  function indexed() {
    let opening;
    function db() {
      if (!opening) opening = new Promise((resolve,reject) => {
        if (!root.indexedDB) return reject(new Error('This browser cannot keep recovery audio.'));
        const request = root.indexedDB.open('leo-audio',1);
        request.onupgradeneeded = () => request.result.createObjectStore('chunks',{keyPath:'key'});
        request.onsuccess = () => resolve(request.result);
        request.onerror = () => reject(request.error);
      });
      return opening;
    }
    async function transaction(mode,work) {
      const database = await db();
      return new Promise((resolve,reject) => {
        const tx = database.transaction('chunks',mode); let result;
        work(tx.objectStore('chunks'),(value) => { result=value; });
        tx.oncomplete = () => resolve(result);
        tx.onerror = () => reject(tx.error || new Error('Recovery audio could not be saved.'));
        tx.onabort = () => reject(tx.error || new Error('Recovery audio storage is full.'));
      });
    }
    return {
      put: (item) => !Number.isSafeInteger(item.seq) || item.seq<0 || item.seq>=MOST_CHUNKS || !item.bytes || item.bytes.byteLength>640000 ? Promise.reject(new Error('Recovery audio reached its limit. Stop and save this recording.')) : transaction('readwrite',(store) => store.put({...item,key:`${item.id}:${String(item.seq).padStart(20,'0')}`})),
      remove: (item) => transaction('readwrite',(store) => store.delete(item.key || `${item.id}:${String(item.seq).padStart(20,'0')}`)),
      list: (id,limit=MOST_CHUNKS) => transaction('readonly',(store,done) => { const out=[]; const range=id ? root.IDBKeyRange.bound(`${id}:`,`${id}:~`) : undefined; const request=store.openCursor(range); request.onsuccess=() => { const cursor=request.result; if (!cursor || out.length>=limit) return done(out); out.push(cursor.value); cursor.continue(); }; }),
      count: (id) => transaction('readonly',(store,done) => { const request=store.count(root.IDBKeyRange.bound(`${id}:`,`${id}:~`)); request.onsuccess=() => done(request.result); }),
    };
  }
  function memory() {
    const items = new Map();
    const key = (item) => item.key || `${item.id}:${String(item.seq).padStart(20, '0')}`;
    const mine = (id) => [...items.values()].filter((item) => item.id === id).sort((a, b) => a.seq - b.seq);
    return {
      put: async (item) => {
        if (!Number.isSafeInteger(item.seq) || item.seq < 0 || item.seq >= MOST_CHUNKS || !item.bytes || item.bytes.byteLength > 640000) throw new Error('Recovery audio reached its limit. Stop and save this recording.');
        items.set(key(item), { ...item, key: key(item) });
      },
      remove: async (item) => { items.delete(key(item)); },
      list: async (id, limit = MOST_CHUNKS) => mine(id).slice(0, limit),
      count: async (id) => mine(id).length,
    };
  }
  function pick() {
    return root.indexedDB ? indexed() : memory();
  }
  async function spool(storage,id,seq,chunks,encode) {
    let at=0;
    try {
      for (;at<Math.min(chunks.length,MOST_CHUNKS);at++) await storage.put({id,seq:seq+at,bytes:encode(chunks[at])});
      if (at<chunks.length) throw new Error('Recovery audio reached its limit. Stop and save this recording.');
      return {next:seq+at,remaining:[]};
    } catch(error) { return {next:seq+at,remaining:chunks.slice(at),error}; }
  }
  async function drain(storage,id,send,limit=MOST_CHUNKS) {
    let count=0;
    while (count<Math.min(limit,MOST_CHUNKS)) {
      const items=await storage.list(id,1); const item=items[0];
      if (!item) return count;
      const next=await send(item);
      if (next <= item.seq) throw new Error('Leo did not acknowledge the audio.');
      await storage.remove(item); count++;
    }
    return count;
  }
  root.leoAudioQueue={indexed,memory,pick,spool,drain};
})(typeof window !== 'undefined' ? window : globalThis);

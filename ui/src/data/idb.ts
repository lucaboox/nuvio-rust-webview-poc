const DB_NAME = "nuvio-desktop-ui";
const STORE = "key-value";
let databasePromise: Promise<IDBDatabase> | null = null;

function database(): Promise<IDBDatabase> {
  if (databasePromise) return databasePromise;

  databasePromise = new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => {
      const db = request.result;
      db.onversionchange = () => {
        db.close();
        databasePromise = null;
      };
      resolve(db);
    };
    request.onerror = () => {
      databasePromise = null;
      reject(request.error);
    };
    request.onblocked = () => {
      databasePromise = null;
      reject(new Error("Nuvio desktop storage is blocked."));
    };
  });
  return databasePromise;
}

export async function getValue<T>(key: string): Promise<T | null> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readonly");
    const request = tx.objectStore(STORE).get(key);
    request.onsuccess = () => resolve((request.result as T | undefined) ?? null);
    request.onerror = () => reject(request.error);
  });
}

export async function setValue<T>(key: string, value: T): Promise<void> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(value, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

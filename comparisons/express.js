// Express.js equivalent — same contacts CRUD + HTML UI
// Requires: npm install express better-sqlite3

const express = require('express');
const Database = require('better-sqlite3');
const app = express();
const db = new Database('contacts.db');

// Setup
app.use(express.json());
app.use(express.urlencoded({ extended: true }));
db.exec(`CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    phone TEXT NOT NULL
)`);

// Create
app.post('/create', (req, res) => {
    const { name, email, phone } = req.body;
    const result = db.prepare('INSERT INTO contacts (name, email, phone) VALUES (?, ?, ?)').run(name, email, phone);
    res.json({ id: result.lastInsertRowid, name, email, phone });
});

app.get('/create', (req, res) => {
    const { name, email, phone } = req.query;
    const result = db.prepare('INSERT INTO contacts (name, email, phone) VALUES (?, ?, ?)').run(name, email, phone);
    const rows = db.prepare('SELECT * FROM contacts ORDER BY id').all();
    let html = '';
    for (const c of rows) {
        html += `<tr><td>${c.name}</td><td>${c.email}</td><td>${c.phone}</td>
            <td><a class="btn btn-sm btn-danger" hx-post="/delete/${c.id}" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>`;
    }
    res.send(html);
});

// Read
app.get('/get/:id', (req, res) => {
    const contact = db.prepare('SELECT * FROM contacts WHERE id = ?').get(req.params.id);
    if (!contact) return res.status(404).json({ error: 'not found' });
    res.json(contact);
});

// List
app.get('/list', (req, res) => {
    const contacts = db.prepare('SELECT * FROM contacts ORDER BY id').all();
    res.json(contacts);
});

// Search
app.get('/search/:q', (req, res) => {
    const contacts = db.prepare('SELECT * FROM contacts WHERE name LIKE ?').all(`%${req.params.q}%`);
    res.json(contacts);
});

// Delete
app.post('/delete/:id', (req, res) => {
    const result = db.prepare('DELETE FROM contacts WHERE id = ?').run(req.params.id);
    if (result.changes === 0) return res.status(404).json({ error: 'not found' });
    res.send('');
});

// HTML Page
app.get('/', (req, res) => {
    const contacts = db.prepare('SELECT * FROM contacts ORDER BY id').all();
    let rows = '';
    for (const c of contacts) {
        rows += `<tr><td>${c.name}</td><td>${c.email}</td><td>${c.phone}</td>
            <td><a class="btn btn-sm btn-danger" hx-post="/delete/${c.id}" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>`;
    }
    res.send(`<html><head><script src="https://unpkg.com/htmx.org@1.9.12"></script>
        <style>*{margin:0;padding:0;box-sizing:border-box;font-family:system-ui}body{background:#0f172a;color:#e2e8f0;padding:2rem}
        h1{font-size:2rem;margin-bottom:1rem}table{width:100%;border-collapse:collapse;background:#1e293b;border-radius:8px;overflow:hidden}
        th{background:#334155;padding:.75rem;text-align:left;font-size:.75rem;text-transform:uppercase;color:#94a3b8}
        td{padding:.75rem;border-top:1px solid #334155}input{padding:.5rem;background:#1e293b;border:1px solid #475569;border-radius:6px;color:#e2e8f0}
        .btn{padding:.4rem .8rem;background:#6366f1;color:white;border:none;border-radius:6px;cursor:pointer;font-weight:600;text-decoration:none}
        .btn-danger{background:#ef4444}form{display:flex;gap:.5rem;margin-bottom:1.5rem}</style></head>
        <body><h1>Contacts</h1>
        <form hx-get="/create" hx-target="#list" hx-swap="innerHTML" hx-on::after-request="this.reset()">
            <input name="name" placeholder="Nom" required><input name="email" placeholder="Email" required>
            <input name="phone" placeholder="Tel" required><button class="btn">Ajouter</button></form>
        <table><thead><tr><th>Nom</th><th>Email</th><th>Tel</th><th></th></tr></thead>
        <tbody id="list">${rows}</tbody></table></body></html>`);
});

app.listen(8080, () => console.log('listening on http://localhost:8080'));

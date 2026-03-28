# Flask equivalent — same contacts CRUD + HTML UI
# Requires: pip install flask

from flask import Flask, request, jsonify
import sqlite3, os

app = Flask(__name__)
DB = 'contacts.db'

def get_db():
    conn = sqlite3.connect(DB)
    conn.row_factory = sqlite3.Row
    return conn

def init_db():
    db = get_db()
    db.execute('''CREATE TABLE IF NOT EXISTS contacts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        email TEXT NOT NULL,
        phone TEXT NOT NULL
    )''')
    db.commit()
    db.close()

init_db()

@app.route('/create', methods=['POST'])
def create():
    data = request.json or request.form
    name, email, phone = data['name'], data['email'], data['phone']
    db = get_db()
    cursor = db.execute('INSERT INTO contacts (name, email, phone) VALUES (?, ?, ?)', (name, email, phone))
    db.commit()
    id = cursor.lastrowid
    db.close()
    return jsonify({'id': id, 'name': name, 'email': email, 'phone': phone})

@app.route('/create', methods=['GET'])
def create_get():
    name = request.args.get('name', '')
    email = request.args.get('email', '')
    phone = request.args.get('phone', '')
    db = get_db()
    db.execute('INSERT INTO contacts (name, email, phone) VALUES (?, ?, ?)', (name, email, phone))
    db.commit()
    rows = db.execute('SELECT * FROM contacts ORDER BY id').fetchall()
    db.close()
    html = ''
    for c in rows:
        html += f'<tr><td>{c["name"]}</td><td>{c["email"]}</td><td>{c["phone"]}</td>'
        html += f'<td><a class="btn btn-sm btn-danger" hx-post="/delete/{c["id"]}" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>'
    return html

@app.route('/get/<int:id>')
def get_contact(id):
    db = get_db()
    contact = db.execute('SELECT * FROM contacts WHERE id = ?', (id,)).fetchone()
    db.close()
    if not contact:
        return jsonify({'error': 'not found'}), 404
    return jsonify(dict(contact))

@app.route('/list')
def list_contacts():
    db = get_db()
    contacts = db.execute('SELECT * FROM contacts ORDER BY id').fetchall()
    db.close()
    return jsonify([dict(c) for c in contacts])

@app.route('/search/<q>')
def search(q):
    db = get_db()
    contacts = db.execute('SELECT * FROM contacts WHERE name LIKE ?', (f'%{q}%',)).fetchall()
    db.close()
    return jsonify([dict(c) for c in contacts])

@app.route('/delete/<int:id>', methods=['POST'])
def delete(id):
    db = get_db()
    cursor = db.execute('DELETE FROM contacts WHERE id = ?', (id,))
    db.commit()
    db.close()
    if cursor.rowcount == 0:
        return jsonify({'error': 'not found'}), 404
    return ''

@app.route('/')
def index():
    db = get_db()
    contacts = db.execute('SELECT * FROM contacts ORDER BY id').fetchall()
    db.close()
    rows = ''
    for c in contacts:
        rows += f'<tr><td>{c["name"]}</td><td>{c["email"]}</td><td>{c["phone"]}</td>'
        rows += f'<td><a class="btn btn-sm btn-danger" hx-post="/delete/{c["id"]}" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>'
    return f'''<html><head><script src="https://unpkg.com/htmx.org@1.9.12"></script>
        <style>*{{margin:0;padding:0;box-sizing:border-box;font-family:system-ui}}body{{background:#0f172a;color:#e2e8f0;padding:2rem}}
        h1{{font-size:2rem;margin-bottom:1rem}}table{{width:100%;border-collapse:collapse;background:#1e293b;border-radius:8px;overflow:hidden}}
        th{{background:#334155;padding:.75rem;text-align:left;font-size:.75rem;text-transform:uppercase;color:#94a3b8}}
        td{{padding:.75rem;border-top:1px solid #334155}}input{{padding:.5rem;background:#1e293b;border:1px solid #475569;border-radius:6px;color:#e2e8f0}}
        .btn{{padding:.4rem .8rem;background:#6366f1;color:white;border:none;border-radius:6px;cursor:pointer;font-weight:600;text-decoration:none}}
        .btn-danger{{background:#ef4444}}form{{display:flex;gap:.5rem;margin-bottom:1.5rem}}</style></head>
        <body><h1>Contacts</h1>
        <form hx-get="/create" hx-target="#list" hx-swap="innerHTML" hx-on::after-request="this.reset()">
            <input name="name" placeholder="Nom" required><input name="email" placeholder="Email" required>
            <input name="phone" placeholder="Tel" required><button class="btn">Ajouter</button></form>
        <table><thead><tr><th>Nom</th><th>Email</th><th>Tel</th><th></th></tr></thead>
        <tbody id="list">{rows}</tbody></table></body></html>'''

if __name__ == '__main__':
    app.run(port=8080)

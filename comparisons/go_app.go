// Go + net/http + SQLite equivalent — same contacts CRUD + HTML UI
// Requires: go get github.com/mattn/go-sqlite3

package main

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"

	_ "github.com/mattn/go-sqlite3"
)

var db *sql.DB

type Contact struct {
	ID    int    `json:"id"`
	Name  string `json:"name"`
	Email string `json:"email"`
	Phone string `json:"phone"`
}

func main() {
	var err error
	db, err = sql.Open("sqlite3", "contacts.db")
	if err != nil {
		panic(err)
	}
	db.Exec(`CREATE TABLE IF NOT EXISTS contacts (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		name TEXT NOT NULL,
		email TEXT NOT NULL,
		phone TEXT NOT NULL
	)`)

	http.HandleFunc("/create", handleCreate)
	http.HandleFunc("/get/", handleGet)
	http.HandleFunc("/list", handleList)
	http.HandleFunc("/search/", handleSearch)
	http.HandleFunc("/delete/", handleDelete)
	http.HandleFunc("/", handleIndex)

	fmt.Println("listening on http://localhost:8080")
	http.ListenAndServe(":8080", nil)
}

func handleCreate(w http.ResponseWriter, r *http.Request) {
	name := r.URL.Query().Get("name")
	email := r.URL.Query().Get("email")
	phone := r.URL.Query().Get("phone")
	if name == "" {
		r.ParseForm()
		name = r.FormValue("name")
		email = r.FormValue("email")
		phone = r.FormValue("phone")
	}
	result, _ := db.Exec("INSERT INTO contacts (name, email, phone) VALUES (?, ?, ?)", name, email, phone)
	id, _ := result.LastInsertId()

	if r.Method == "GET" {
		rows, _ := db.Query("SELECT id, name, email, phone FROM contacts ORDER BY id")
		defer rows.Close()
		var html string
		for rows.Next() {
			var c Contact
			rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone)
			html += fmt.Sprintf(`<tr><td>%s</td><td>%s</td><td>%s</td><td><a class="btn btn-sm btn-danger" hx-post="/delete/%d" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>`, c.Name, c.Email, c.Phone, c.ID)
		}
		w.Write([]byte(html))
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(Contact{ID: int(id), Name: name, Email: email, Phone: phone})
}

func handleGet(w http.ResponseWriter, r *http.Request) {
	id := strings.TrimPrefix(r.URL.Path, "/get/")
	var c Contact
	err := db.QueryRow("SELECT id, name, email, phone FROM contacts WHERE id = ?", id).Scan(&c.ID, &c.Name, &c.Email, &c.Phone)
	if err != nil {
		w.WriteHeader(404)
		w.Write([]byte(`{"error":"not found"}`))
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(c)
}

func handleList(w http.ResponseWriter, r *http.Request) {
	rows, _ := db.Query("SELECT id, name, email, phone FROM contacts ORDER BY id")
	defer rows.Close()
	var contacts []Contact
	for rows.Next() {
		var c Contact
		rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone)
		contacts = append(contacts, c)
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(contacts)
}

func handleSearch(w http.ResponseWriter, r *http.Request) {
	q := strings.TrimPrefix(r.URL.Path, "/search/")
	rows, _ := db.Query("SELECT id, name, email, phone FROM contacts WHERE name LIKE ?", "%"+q+"%")
	defer rows.Close()
	var contacts []Contact
	for rows.Next() {
		var c Contact
		rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone)
		contacts = append(contacts, c)
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(contacts)
}

func handleDelete(w http.ResponseWriter, r *http.Request) {
	id := strings.TrimPrefix(r.URL.Path, "/delete/")
	result, _ := db.Exec("DELETE FROM contacts WHERE id = ?", id)
	n, _ := result.RowsAffected()
	if n == 0 {
		w.WriteHeader(404)
		w.Write([]byte(`{"error":"not found"}`))
		return
	}
	w.Write([]byte(""))
}

func handleIndex(w http.ResponseWriter, r *http.Request) {
	rows, _ := db.Query("SELECT id, name, email, phone FROM contacts ORDER BY id")
	defer rows.Close()
	var html string
	for rows.Next() {
		var c Contact
		rows.Scan(&c.ID, &c.Name, &c.Email, &c.Phone)
		html += fmt.Sprintf(`<tr><td>%s</td><td>%s</td><td>%s</td><td><a class="btn btn-sm btn-danger" hx-post="/delete/%d" hx-target="closest tr" hx-swap="outerHTML swap:150ms">X</a></td></tr>`, c.Name, c.Email, c.Phone, c.ID)
	}
	w.Write([]byte(fmt.Sprintf(`<html><head><script src="https://unpkg.com/htmx.org@1.9.12"></script>
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
		<tbody id="list">%s</tbody></table></body></html>`, html)))
}

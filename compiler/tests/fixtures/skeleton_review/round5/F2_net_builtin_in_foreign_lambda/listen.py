import socket,sys,time
s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR,1); s.bind(("127.0.0.1",47392)); s.listen(5); s.settimeout(25)
try:
    while True:
        c,a=s.accept(); c.settimeout(1)
        try: d=c.recv(200)
        except Exception as e: d=b''
        open("conn.log","a").write("CONNECTED %r\n"%d[:80]); c.close()
except Exception as e: pass

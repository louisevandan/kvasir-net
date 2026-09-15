"""OUTER client for the public P4E3 envelope. Model bytes remain opaque."""
import socket
import struct
import time
import uuid


def text(value):
    data=value.encode("utf-8")
    return struct.pack("<I",len(data))+data


def endpoint(value):
    kind, *fields=value
    if kind == 0:
        return b"\0"+text(fields[0])
    return bytes([kind])+text(fields[0])+text(fields[1])+struct.pack("<Q",fields[2])


class Reader:
    def __init__(self,data):
        self.data,self.pos=data,0
    def take(self,n):
        value=self.data[self.pos:self.pos+n]
        if len(value)!=n:
            raise ValueError("truncated P4 envelope")
        self.pos+=n
        return value
    def num(self,fmt):
        return struct.unpack(fmt,self.take(struct.calcsize(fmt)))[0]
    def text(self):
        return self.take(self.num("<I")).decode("utf-8")
    def optional(self,read):
        flag=self.num("B")
        if flag not in (0,1):
            raise ValueError("invalid optional flag")
        return read() if flag else None
    def endpoint(self):
        kind=self.num("B")
        if kind==0:
            return (0,self.text())
        if kind not in (1,2):
            raise ValueError("invalid endpoint")
        return (kind,self.text(),self.text(),self.num("<Q"))


class Client:
    def __init__(self,host,port,timeout=120):
        self.address=f"tcp://{host}:{port}"
        self.socket=socket.create_connection((host,port),timeout=timeout)
        self.socket.setsockopt(socket.IPPROTO_TCP,socket.TCP_NODELAY,1)
        self.socket.settimeout(timeout)
        self.outer=(2,self.address,uuid.uuid4().hex,1)
        self.sequence=0
        self.sent_bytes=self.received_bytes=0
        self.trace=[]
        self.finishing=False
    def send(self,target,content,payload,adapter=None):
        if self.finishing:
            raise RuntimeError("P4 connection is finishing")
        self.sequence+=1
        event_id=f"{self.outer[2]}:{self.sequence}"
        env=struct.pack("<H",3)+text(event_id)+text(event_id)+b"\0"+endpoint(self.outer)+endpoint(target)
        env+=b"\1"+endpoint(self.outer)[1:]+b"\0"+struct.pack("<Q",self.sequence)
        env+=b"\1"+struct.pack("<Q",int(time.time()*1000)+120000)
        env+=(b"\1"+text(adapter)) if adapter else b"\0"
        env+=text(content)
        data=b"P4E3"+struct.pack("<II",len(env),len(payload))+env+payload
        self.socket.sendall(struct.pack("<I",len(data))+data)
        self.sent_bytes+=len(data)+4
        return event_id
    def exact(self,n):
        result=bytearray()
        while len(result)<n:
            block=self.socket.recv(n-len(result))
            if not block:
                raise EOFError("P4 connection closed")
            result.extend(block)
        return bytes(result)
    def receive(self):
        size,=struct.unpack("<I",self.exact(4))
        if size>40*1024*1024:
            raise ValueError("P4 response exceeds client bound")
        data=self.exact(size)
        if data[:4]!=b"P4E3":
            raise ValueError("invalid P4 magic")
        e,p=struct.unpack("<II",data[4:12])
        if len(data)!=12+e+p:
            raise ValueError("invalid P4 frame length")
        r=Reader(data[12:12+e])
        if r.num("<H")!=3:
            raise ValueError("invalid P4 version")
        meta={"event_id":r.text(),"correlation":r.text(),"causation":r.optional(r.text),"source":r.endpoint(),"target":r.endpoint()}
        meta["return_route"]=r.optional(lambda:(2,r.text(),r.text(),r.num("<Q")))
        meta.update(event_class=r.num("B"),sequence=r.num("<Q"),deadline=r.optional(lambda:r.num("<Q")),adapter=r.optional(r.text),content=r.text())
        if (r.pos!=e or meta["target"]!=self.outer
                or meta["return_route"]!=self.outer
                or (meta["source"][0]==2 and meta["source"]!=self.outer)):
            raise ValueError("response envelope mismatch")
        self.received_bytes+=size+4
        self.trace.append({**meta,"bytes":size+4})
        return meta,data[12+e:]
    def exchange(self,target,content,payload,adapter=None):
        event_id=self.send(target,content,payload,adapter)
        meta,body=self.receive()
        if meta["correlation"]!=event_id:
            raise ValueError("response correlation mismatch")
        return meta,body
    def close(self):
        self.socket.close()

    def finish(self,timeout=10):
        """Retire socket routes after expected outputs; not request/KV settlement."""
        if self.finishing:
            raise RuntimeError("P4 connection already finishing")
        self.finishing=True
        previous=self.socket.gettimeout()
        try:
            self.socket.settimeout(timeout)
            self.socket.sendall(struct.pack("<I",0))
            self.sent_bytes+=4
            ack=self.exact(4)
            self.received_bytes+=4
            if ack!=bytes(4):
                raise ValueError(f"unexpected output before P4 finish ACK: {ack.hex()}")
        finally:
            self.socket.settimeout(previous)

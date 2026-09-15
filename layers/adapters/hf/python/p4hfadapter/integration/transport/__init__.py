"""OUTER client for the public P4E3 envelope. Model bytes remain opaque."""
import socket
import struct
import time
import uuid
import hashlib


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
        self.finish_unexpected_output=bytearray()
        self.hop_enabled=True
        self.hop_generation=self.outer[3]
        self.hop_attempt=1
        self.hop_outstanding={}
        self.hop_received={}
        self.hop_pending=[]
        self.hop_max_outstanding=256
        self.hop_sender_id=self.address+"#"+self.outer[2]
        hello=b"P4H1"+struct.pack("<HHB",1,0,1)+text(self.hop_sender_id)
        hello+=struct.pack("<QIQ",self.hop_generation,self.hop_max_outstanding,64*1024*1024)
        self._send_frame(hello)
        ack=self._hop_decode(self._read_frame())
        if ack[0]!="hello_ack" or ack[1]!=self.hop_generation:
            raise ValueError("P4 hop hello acknowledgement mismatch")
        self.hop_max_outstanding=min(self.hop_max_outstanding,ack[4])
        self.hop_peer_sender_id,self.hop_peer_generation=ack[2],ack[3]

    def _send_frame(self,data):
        self.socket.sendall(struct.pack("<I",len(data))+data)
        self.sent_bytes+=len(data)+4

    def _read_frame(self):
        size,=struct.unpack("<I",self.exact(4))
        if size==0 or size>40*1024*1024:
            raise ValueError("P4 response exceeds client bound")
        data=self.exact(size)
        self.received_bytes+=size+4
        return data

    def _hop_decode(self,data):
        if data[:4]!=b"P4H1" or len(data)<9:
            raise ValueError("invalid P4 hop magic")
        version,reserved,kind=struct.unpack("<HHB",data[4:9])
        if version!=1 or reserved!=0:
            raise ValueError("unsupported P4 hop version")
        r=Reader(data[9:])
        if kind==2:
            accepted=r.num("<Q"); sender=r.text(); generation=r.num("<Q")
            maximum=r.num("<I"); receipt_bytes=r.num("<Q")
            value=("hello_ack",accepted,sender,generation,maximum,receipt_bytes)
        elif kind==3:
            attempt=r.num("<Q"); digest=r.take(32); size=r.num("<I")
            event=r.take(size)
            if hashlib.sha256(event).digest()!=digest:
                raise ValueError("P4 hop data digest mismatch")
            value=("data",attempt,digest,event)
        elif kind in (4,7):
            attempt=r.num("<Q"); digest=r.take(32); status=r.num("B"); detail=r.text()
            if status not in (1,2,3,4):
                raise ValueError("invalid P4 hop receipt status")
            value=(("receipt" if kind==4 else "query_result"),attempt,digest,status,detail)
        elif kind==5:
            value=("receipt_ack",r.text(),r.num("<Q"),r.num("<Q"),r.take(32))
        elif kind==6:
            value=("query",r.text(),r.num("<Q"),r.num("<Q"),r.take(32))
        else:
            raise ValueError("unexpected P4 hop frame")
        if r.pos!=len(r.data):
            raise ValueError("trailing P4 hop bytes")
        return value

    def _hop_receipt(self,kind,attempt,digest,status=1,detail=""):
        tag=4 if kind=="receipt" else 7
        return b"P4H1"+struct.pack("<HHBQ",1,0,tag,attempt)+digest+bytes([status])+text(detail)

    def _hop_progress(self):
        frame=self._hop_decode(self._read_frame())
        kind=frame[0]
        if kind=="receipt":
            _,attempt,digest,status,detail=frame
            existing=self.hop_outstanding.get(attempt)
            if existing is None or existing[0]!=digest:
                raise ValueError("P4 hop receipt conflicts with outstanding request")
            if status!=1:
                raise RuntimeError(f"P4 hop receipt refused request: status={status} detail={detail}")
            del self.hop_outstanding[attempt]
            ack=b"P4H1"+struct.pack("<HHB",1,0,5)+text(self.hop_sender_id)
            ack+=struct.pack("<QQ",self.hop_generation,attempt)+digest
            self._send_frame(ack)
        elif kind=="data":
            _,attempt,digest,event=frame
            existing=self.hop_received.get(attempt)
            if existing is not None:
                status=1 if existing==digest else 3
                self._send_frame(self._hop_receipt("receipt",attempt,digest,status,
                    "" if status==1 else "attempt digest differs"))
                return
            self.hop_received[attempt]=digest
            self.hop_pending.append(event)
            self._send_frame(self._hop_receipt("receipt",attempt,digest))
        elif kind=="receipt_ack":
            _,sender,generation,attempt,digest=frame
            if (sender==self.hop_peer_sender_id and generation==self.hop_peer_generation
                    and self.hop_received.get(attempt)==digest):
                del self.hop_received[attempt]
        elif kind=="query":
            _,sender,generation,attempt,digest=frame
            existing=self.hop_received.get(attempt)
            status=4 if existing is None else (1 if existing==digest else 3)
            detail="receipt is not pinned" if status==4 else ("attempt digest differs" if status==3 else "")
            self._send_frame(self._hop_receipt("query_result",attempt,digest,status,detail))
        else:
            raise ValueError(f"unexpected P4 hop frame {kind}")
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
        if getattr(self,"hop_enabled",False):
            while len(self.hop_outstanding)>=self.hop_max_outstanding:
                self._hop_progress()
            attempt=self.hop_attempt
            self.hop_attempt+=1
            digest=hashlib.sha256(data).digest()
            self.hop_outstanding[attempt]=(digest,event_id,data)
            frame=b"P4H1"+struct.pack("<HHBQ",1,0,3,attempt)+digest+struct.pack("<I",len(data))+data
            self._send_frame(frame)
        else:
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
        if getattr(self,"hop_enabled",False):
            while not self.hop_pending:
                self._hop_progress()
            data=self.hop_pending.pop(0)
            size=len(data)
        else:
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
        if not getattr(self,"hop_enabled",False):
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
            if getattr(self,"hop_enabled",False):
                while self.hop_outstanding or self.hop_received:
                    self._hop_progress()
                if self.hop_pending:
                    raise ValueError("unconsumed events before P4 finish")
            self.socket.sendall(struct.pack("<I",0))
            self.sent_bytes+=4
            while len(self.finish_unexpected_output)<4:
                block=self.socket.recv(4-len(self.finish_unexpected_output))
                if not block:
                    raise EOFError(
                        "P4 connection closed during finish response: "
                        f"buffered_bytes={len(self.finish_unexpected_output)}"
                    )
                self.finish_unexpected_output.extend(block)
                self.received_bytes+=len(block)
            ack=bytes(self.finish_unexpected_output)
            if ack!=bytes(4):
                size,=struct.unpack("<I",ack)
                if size>40*1024*1024:
                    raise ValueError(
                        "unexpected output before P4 finish ACK exceeds client bound: "
                        f"declared_bytes={size} buffered_bytes=4"
                    )
                while len(self.finish_unexpected_output)<size+4:
                    block=self.socket.recv(size+4-len(self.finish_unexpected_output))
                    if not block:
                        raise EOFError(
                            "P4 connection closed during unexpected finish output: "
                            f"buffered_bytes={len(self.finish_unexpected_output)}"
                        )
                    self.finish_unexpected_output.extend(block)
                    self.received_bytes+=len(block)
                raise ValueError(
                    "unexpected output before P4 finish ACK: "
                    f"frame_bytes={size+4} buffered_bytes={len(self.finish_unexpected_output)}"
                )
            self.finish_unexpected_output.clear()
        finally:
            self.socket.settimeout(previous)

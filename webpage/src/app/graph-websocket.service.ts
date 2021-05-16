import { Injectable } from '@angular/core';
import { Observable, Observer, Subject } from 'rxjs';
import { Row } from './room-list/room-list.component';


export interface WsMessage {
  node: Row;
  link: any;
}
@Injectable({
  providedIn: 'root'
})
export class GraphWebsocketService {
  private subject!: Subject<MessageEvent<WsMessage>>;

  constructor() { }

  public connect(): Subject<MessageEvent<WsMessage>> {
    if (!this.subject) {
      this.subject = this.create('wss://serverstats.nordgedanken.dev/ws');
      console.log("Successfully connected: " + '/ws');
    }
    return this.subject;
  }

  private create(url: string): Subject<MessageEvent<WsMessage>> {
    let ws = new WebSocket(url);

    let observable = new Observable((obs: Observer<MessageEvent<WsMessage>>) => {
      ws.onmessage = obs.next.bind(obs);
      ws.onerror = obs.error.bind(obs);
      ws.onclose = obs.complete.bind(obs);
      return ws.close.bind(ws);
    });
    let observer = {
      next: (data: any) => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify(data));
        }
      },
      error: (err: any) => console.error('Websocket got an error: ' + err),
      complete: () => { },
    };
    return Subject.create(observer, observable);
  }
}

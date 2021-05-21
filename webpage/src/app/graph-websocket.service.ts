import { Injectable } from '@angular/core';
import { EMPTY, Observable, Subject, timer } from 'rxjs';
import { Row } from './room-list/room-list.component';
import { retryWhen, tap, delayWhen, switchAll, catchError } from 'rxjs/operators';
import { webSocket, WebSocketSubject } from 'rxjs/webSocket';


export interface WsMessage {
  node: Row;
  link: any;
}
@Injectable({
  providedIn: 'root'
})
export class GraphWebsocketService {
  private socket?: WebSocketSubject<WsMessage>;
  public messages?: Observable<WsMessage>;

  constructor() { }

  /**
  * Creates a new WebSocket subject and send it to the messages subject
  * @param cfg if true the observable will be retried.
  */
  public connect(cfg: { reconnect: boolean } = { reconnect: true }): void {

    if (!this.socket || this.socket.closed) {
      this.socket = this.getNewWebSocket();
      const messages = this.socket.pipe(cfg.reconnect ? this.reconnect : o => o,
        tap({
          error: error => console.log(error),
        }), catchError(_ => EMPTY))
      this.messages = messages;
    }
  }

  /**
   * Retry a given observable by a time span
   * @param observable the observable to be retried
   */
  private reconnect(observable: Observable<any>): Observable<any> {
    return observable.pipe(retryWhen(errors => errors.pipe(tap(val => console.log('[WS] Try to reconnect', val)),
      delayWhen(_ => timer(5)))));
  }


  close() {
    this.socket?.complete();
    this.socket = undefined;
  }

  sendMessage(msg: any) {
    this.socket?.next(msg);
  }

  /**
   * Return a custom WebSocket subject which reconnects after failure
   */
  private getNewWebSocket(): WebSocketSubject<WsMessage> {
    return webSocket({
      url: 'wss://serverstats.nordgedanken.dev/ws',
      openObserver: {
        next: () => {
          console.log('[WS]: connection ok');
        }
      },
      closeObserver: {
        next: () => {
          console.log('[WS]: connection closed');
          this.socket = undefined;
          this.connect({ reconnect: true });
        }
      },

    });
  }
}
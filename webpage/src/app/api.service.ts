import { HttpClient, HttpHeaders } from '@angular/common/http';
import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';
import { GraphWebsocketService, WsMessage } from './graph-websocket.service';

export interface Row {
  id: string;
  name: string;
  alias: string;
  avatar: string;
  topic: string;
  weight: string;
  is_space: boolean;
  updated?: boolean;
}
export interface APIData {
  nodes: Row[],
  links: any[]
}
@Injectable({
  providedIn: 'root'
})
export class ApiService {
  public data?: APIData;
  requested = false;
  constructor(private websocket: GraphWebsocketService, private http: HttpClient) {

  }

  public getDataUpdates(): Observable<APIData> {
    return new Observable((observer => {
      if (this.data == null && !this.requested) {
        this.requested = true;
        this.http.get<APIData>('/relations', { headers: new HttpHeaders({ 'Content-Type': 'application/json', }) }).subscribe((data: APIData) => {
          this.data = data;
          observer.next(this.data);

          this.websocket.connect();

          this.websocket.messages?.subscribe((event: WsMessage) => {
            const data = event;
            let graph_data = this.data;
            let nodes_new = this.difference_nodes(graph_data?.nodes as Row[], data.node);
            let links_new = this.difference_links(graph_data?.links as any[], data.link);
            if (nodes_new.length !== 0 || links_new.length !== 0) {
              let new_data = {
                nodes: [...graph_data?.nodes as object[], ...nodes_new],
                links: [...graph_data?.links as object[], ...links_new]
              };
              this.data = new_data as APIData;
              observer.next(this.data);
            }
          });
        });
      }


    }));
  }

  difference_nodes(old: Row[], newd: Row) {
    if (!old.some(node_old => node_old.id == newd.id)) {
      return [newd];
    } else {
      return [];
    }
  }

  difference_links(old: any[], newd: any) {
    if (!old.some(link_old => link_old.source.id == newd.source && newd.target == link_old.target.id)) {
      return [newd];
    } else {
      return [];
    }
  }

}

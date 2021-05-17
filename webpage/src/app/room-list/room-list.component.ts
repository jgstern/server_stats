import { Component, OnInit, ViewChild } from '@angular/core';
import { ColumnMode, DatatableComponent } from '@swimlane/ngx-datatable';
import { HttpClient, HttpHeaders } from '@angular/common/http';
import Autolinker from 'autolinker';

export interface Row {
  id: string;
  name: string;
  alias: string;
  avatar: string;
  topic: string;
  weight: string;
}
export interface APIData {
  nodes: Row[],
  links: any[]
}
@Component({
  selector: 'app-room-list',
  templateUrl: './room-list.component.html',
  styleUrls: ['./room-list.component.scss']
})
export class RoomListComponent implements OnInit {
  @ViewChild(DatatableComponent)
  table!: DatatableComponent;

  title = 'server-stats';
  filterColumn = 'name';
  rows: Row[] = [];
  temp: Row[] = [];
  columns = [{ prop: 'name', name: 'Roomname' }, { name: 'Alias' }, { prop: 'room_id', name: 'Room ID' }, { name: 'Topic' }, { prop: 'incoming_links', name: 'Incoming Links' }, { prop: 'outgoing_links', name: 'Outgoing Links' }];
  ColumnMode = ColumnMode;

  constructor(private http: HttpClient) { }

  ngOnInit(): void {
    //TODO use public iphttp://localhost:3332
    this.http.get<APIData>('/relations', { headers: new HttpHeaders({ 'Content-Type': 'application/json', }) }).subscribe((data: APIData) => {
      // cache our list
      data.nodes = data.nodes.map(data => {
        data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
        data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
        return data;
      });
      this.temp = [...data.nodes];
      this.rows = data.nodes;
    });
  }
  truncateText(text: string, length: number) {
    if (text.length <= length) {
      return text;
    }

    return text.substr(0, length) + '\u2026';
  }

  updateSelection(value: string) {
    this.filterColumn = value.toLowerCase();
  }

  updateFilter(event: any) {
    const val = event.target.value.toLowerCase();
    const filterColumn = this.filterColumn;
    // filter our data
    const temp = this.temp.filter(function (d) {
      const indexName = filterColumn as "id" | "name" | "alias" | "avatar" | "topic" | "weight";
      return d[indexName].toLowerCase().indexOf(val) !== -1 || !val;
    });

    // update the rows
    this.rows = temp;
    // Whenever the filter changes, always go back to the first page
    this.table.offset = 0;
  }
}

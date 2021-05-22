import { Component, OnInit, ViewChild } from '@angular/core';
import { ColumnMode, DatatableComponent } from '@swimlane/ngx-datatable';
import Autolinker from 'autolinker';
import { ApiService } from '../api.service';
import Fuse from 'fuse.js'

@Component({
  selector: 'app-link-finder',
  templateUrl: './link-finder.component.html',
  styleUrls: ['./link-finder.component.scss']
})
export class LinkFinderComponent implements OnInit {
  @ViewChild(DatatableComponent)
  table!: DatatableComponent;

  room_name = "";
  links: any[] = [];
  rows: any[] = [];
  temp: any[] = [];
  filterColumn = 'incoming';
  columns = [{ prop: 'name', name: 'Roomname' }, { name: 'Alias' }, { prop: 'room_id', name: 'Room ID' }, { name: 'Topic' }, { prop: 'incoming_links', name: 'Incoming Links' }, { prop: 'outgoing_links', name: 'Outgoing Links' }];
  ColumnMode = ColumnMode;
  first = true;

  constructor(public api: ApiService) { }

  ngOnInit(): void {
    if (this.api.data != null) {

      this.links = this.api.data.links;
      this.temp = this.rows;
      if (this.first) {
        this.rows = this.api.data.nodes;
        this.rows = this.rows.filter(node => this.links.some(value => node["id"] === value["target"])).map(data => {
          if (data.updated === false || data.updated == null) {
            data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
            data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
            data.updated = true;
          }
          return data;
        });
        this.first = false;
      }
    }
    this.api.getDataUpdates().subscribe(data => {
      if (data != null) {
        const nodes = data.nodes;
        this.temp = nodes;
        this.links = data.links;
        if (this.first) {
          this.rows = nodes;
          this.rows = this.rows.filter(node => this.links.some(value => node["id"] === value["target"])).map(data => {
            if (data.updated === false || data.updated == null) {
              data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;
          });
          this.first = false;
        }
      }
    })
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
    const indexName = filterColumn as "incoming" | "outgoing";
    const options = {
      includeScore: true,
      // Search in `author` and in `tags` array
      keys: ['name']
    }

    const fuse = new Fuse(this.temp, options);

    const result = fuse.search(val);
    if (result.length == 0) {
      if (this.api.data != null) {
        const nodes = this.api.data.nodes.map(data => {
          if (data.updated === false || data.updated == null) {
            data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
            data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
            data.updated = true;
          }
          return data;
        });
        this.rows = nodes;
      }

      return;
    }

    result.sort((a, b) => {
      // Compare the 2 scores
      if (a.score!! < b.score!!) return -1;
      if (a.score!! > b.score!!) return 1;
      return 0;
    });

    const room_hash = result[0].item["id"];
    this.room_name = result[0].item["name"];
    if (indexName == "incoming") {
      const links = this.links.filter(link => link["target"] === room_hash);
      this.rows = this.temp.filter(node => links.some(value => node["id"] === value["source"])).map(data => {
        if (data.updated === false || data.updated == null) {
          data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
          data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
          data.updated = true;
        }
        return data;
      });
    } else if (indexName == "outgoing") {
      const links = this.links.filter(link => link["source"] === room_hash);
      this.rows = this.temp.filter(node => links.some(value => node["id"] === value["target"])).map(data => {
        if (data.updated === false || data.updated == null) {
          data.alias = `<a href="https://matrix.to/#/${data.alias}">${data.alias}</a>`;
          data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
          data.updated = true;
        }
        return data;
      });
    }
    // Whenever the filter changes, always go back to the first page
    this.table.offset = 0;
  }

}

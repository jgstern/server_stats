import { AfterViewInit, Component, ElementRef, HostListener, OnInit, ViewChild } from '@angular/core';
import ForceGraph3D, {
  ForceGraph3DInstance
} from "3d-force-graph";
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass';
import * as THREE from 'three';
import { HttpClient, HttpHeaders } from '@angular/common/http';
import Autolinker from 'autolinker';
import { APIData, Row } from '../room-list/room-list.component';
import { GraphWebsocketService } from '../graph-websocket.service';


@Component({
  selector: 'app-three-d-graph',
  templateUrl: './three-d-graph.component.html',
  styleUrls: ['./three-d-graph.component.scss']
})
export class ThreeDGraphComponent implements OnInit, AfterViewInit {
  @ViewChild('graph')
  graph_element!: ElementRef<any>;

  @ViewChild('sidebar')
  sidebar!: ElementRef<any>;

  @ViewChild('link')
  link!: ElementRef<any>;

  @ViewChild('close')
  close!: ElementRef<any>;

  sidebar_header: string = "";
  sidebar_description: string = "";
  avatar: string = "";
  alias_link: string = "";
  alias_link_href: string = "";
  private graph?: ForceGraph3DInstance;
  data?: APIData;

  constructor(private http: HttpClient, private websocket: GraphWebsocketService) {
    websocket.connect().subscribe(event => {
      const data = event.data;
      let graph_data = this.graph?.graphData();
      let nodes_new = this.difference_nodes(graph_data?.nodes as Row[], data.node);
      let links_new = this.difference_links(graph_data?.links as any[], data.link);
      if (nodes_new.length !== 0 || links_new.length !== 0) {
        let new_data = {
          nodes: [...graph_data?.nodes as object[], ...nodes_new],
          links: [...graph_data?.links as object[], ...links_new]
        };
        this.data = new_data as APIData;
        this.graph?.graphData(new_data);
      }
    });
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

  setupGraph() {
    this.sidebar.nativeElement.addEventListener('animationend', (e: { preventDefault: () => void; }) => {
      e.preventDefault();
      this.sidebar.nativeElement.setAttribute('id', 'sidebar');
      if (this.sidebar.nativeElement.classList.contains('close_anim')) {
        this.sidebar.nativeElement.style.removeProperty('display');
        this.sidebar.nativeElement.removeAttribute('animation');
      }
      this.sidebar.nativeElement.removeAttribute('class');

    });
    this.close.nativeElement.addEventListener('click', () => {
      this.sidebar.nativeElement.classList.add('close_anim');
      this.sidebar.nativeElement.setAttribute('animation', 'close_anim');
    })

    const bloomPass = new UnrealBloomPass(new THREE.Vector2(window.innerWidth, window.innerHeight), 0.35, 0, 0.1);
    this.graph!.renderer().toneMappingExposure = Math.pow(1, 4.0);

    this.graph?.onNodeClick((node: any) => {
      if (!this.sidebar.nativeElement.classList.contains('close_anim')) {
        this.sidebar.nativeElement.classList.add('animate__animated', 'animate__fadeInRight');
        this.sidebar.nativeElement.style.display = "block";
      }
      this.sidebar_header = node.name as string;
      this.sidebar_description = Autolinker.link(node.topic, { sanitizeHtml: true });
      if (node.avatar !== "") {
        const avatar_url = node.avatar.replace('mxc://', '').split('/');
        this.avatar = `https://matrix.nordgedanken.dev/_matrix/media/r0/download/${avatar_url[0]}/${avatar_url[1]}`;
      } else { this.avatar = ""; }

      if (node.alias !== "") {
        this.link.nativeElement.style.setProperty('display', 'inline');
        this.alias_link = node.alias;
        this.alias_link_href = `https://matrix.to/#/${encodeURIComponent(node.alias)}`;
      }
    })

    this.graph?.graphData(this.data!).postProcessingComposer().addPass(bloomPass);

    this.graph?.onNodeDrag(() => this.graph?.cooldownTicks(0))
    this.graph?.onNodeDragEnd(() => this.graph?.cooldownTicks(Infinity))

    this.graph?.warmupTicks(0)
      .cooldownTicks(Infinity);
    setTimeout(() => { this.graph?.zoomToFit(400); }, 5000);
    this.windowResize();
  }

  ngAfterViewInit() {
    //TODO use public iphttp://localhost:3332
    this.http.get<APIData>('/relations', { headers: new HttpHeaders({ 'Content-Type': 'application/json', }) }).subscribe((data: APIData) => {
      this.data = data;
      this.setupGraph();
    });
    this.graph = ForceGraph3D()(this.graph_element.nativeElement).warmupTicks(50)
      .backgroundColor('#101020')
      .nodeRelSize(6)
      .nodeAutoColorBy('name')
      .nodeOpacity(0.95)
      .nodeResolution(8)
      .linkColor(() => 'rgba(255,255,255,0.2)')
      .linkOpacity(0.8)
      .linkWidth(2)
      .linkDirectionalParticles(2)
      .linkDirectionalParticleWidth(1)
      .linkDirectionalParticleSpeed(0.003)
      .onNodeHover(node => this.graph_element.nativeElement.style.cursor = node ? 'pointer' : null);
  }


  @HostListener("window:resize")
  public windowResize() {
    const box = this.graph_element.nativeElement.getBoundingClientRect();
    this.graph?.width(box.width);
    this.graph?.height(box.height);
    // @ts-ignore
    this.graph?.controls().handleResize();
  }

  ngOnInit() {
  }

}
